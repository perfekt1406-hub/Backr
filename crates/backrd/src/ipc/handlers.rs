/*
 * ipc/handlers.rs — IPC method dispatch for the backrd daemon.
 *
 * `dispatch` is the single entry point called by the connection handler after
 * deserialising each `IpcRequest`.  It matches on the method name and calls the
 * appropriate handler, returning either a JSON result payload or an `IpcError`.
 *
 * All 26 handlers are implemented here, porting logic faithfully from the original
 * Tauri command implementations in `src-tauri/src/commands/`.  `AppHandle`/`AppState`
 * are replaced with `Arc<DaemonState>` and `IpcBroadcastSink`.  No new behaviour is
 * introduced — each handler is a mechanical translation of the Tauri original.
 *
 * Error mapping: `BackrCommandError` is mapped to `IpcError` via `cmd_err`.
 * Params deserialization: typed structs use `serde_json::from_value`; scalar params
 * use the `str_param` helper.
 * Result serialization: `serde_json::to_value(result)` wrapped via `to_json`.
 */

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::broadcast;

use backr_core::backup::rsync;
use backr_core::backup::ssh;
use backr_core::backup::validate::{validate_relative_path, validate_remote_component};
use backr_core::config::{self, Config};
use backr_core::error::{BackrCommandError, BackrError};
use backr_core::host_config::read_host_dashboard_marker;
use backr_core::host_disk_inventory;
use backr_core::host_trust;
use backr_core::pairing::client::{
    confirm_pair_draft, pair_with_host as do_pair_with_host, PairDraft,
};
use backr_core::pairing::code::PairingSession;
use backr_core::pairing::discovery::{advertise, discover_hosts as do_discover_hosts, DiscoveredHost};
use backr_core::pairing::listener::{gather_host_info, serve};
use backr_core::pairing::PairingRuntime;
use backr_core::progress_sink::SharedProgress;
use backr_core::project_snapshot_cache::{
    self, load_snapshot_cache, remote_cache_key, save_snapshot_cache,
};
use backr_core::scheduler::restart_scheduler;
use tiny_http::Server;

use crate::daemon_state::DaemonState;
use crate::event_sink::IpcBroadcastSink;
use crate::ipc::protocol::{IpcError, IpcEvent};

/// Auto-teardown lifetime for a pairing window. After this duration with no successful
/// pair, the listener closes and mDNS stops advertising.
const PAIRING_TTL_SECS: u64 = 180;

// ---------------------------------------------------------------------------
// Error mapping helpers
// ---------------------------------------------------------------------------

/// Maps a `BackrCommandError` to an `IpcError` for wire transmission.
///
/// # Inputs
///
/// * `e` — structured command error from `backr-core`.
///
/// # Returns
///
/// `IpcError` with the `ErrorKind` debug string as `kind` and the original `message`.
fn cmd_err(e: BackrCommandError) -> IpcError {
    IpcError::new(format!("{:?}", e.kind), e.message)
}

/// Maps a `BackrError` to `IpcError` via the `BackrCommandError` conversion.
///
/// # Inputs
///
/// * `e` — internal core error converted through `BackrCommandError`.
fn core_err(e: BackrError) -> IpcError {
    cmd_err(BackrCommandError::from(e))
}

/// Serialises a `serde::Serialize` value to `serde_json::Value`.
///
/// # Inputs
///
/// * `v` — any serializable value.
///
/// # Returns
///
/// `Ok(Value)` on success; `Err(IpcError)` with kind `"Internal"` on serialisation failure.
fn to_json<T: serde::Serialize>(v: T) -> Result<Value, IpcError> {
    serde_json::to_value(v).map_err(|e| IpcError::new("Internal", e.to_string()))
}

/// Extracts a required string parameter from a `serde_json::Value` params object.
///
/// # Inputs
///
/// * `params` — the raw JSON params value.
/// * `key`    — the field name to look up.
///
/// # Returns
///
/// `Ok(String)` when present; `Err(IpcError)` with `"InvalidInput"` kind when missing.
fn str_param(params: &Value, key: &str) -> Result<String, IpcError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| IpcError::new("InvalidInput", format!("missing field: {key}")))
}

// ---------------------------------------------------------------------------
// Dispatch table
// ---------------------------------------------------------------------------

/// Dispatches an incoming IPC request to the appropriate handler.
///
/// # Parameters
/// - `method`   — The method name string from `IpcRequest::method`.
/// - `params`   — The raw JSON params object from `IpcRequest::params`.
/// - `state`    — Shared daemon state; handlers lock only the fields they need.
/// - `event_tx` — Broadcast sender for progress events (backup, restore commands).
///
/// # Returns
/// `Ok(Value)` on success (serialised into `IpcResponse::result`), or
/// `Err(IpcError)` wrapped into `IpcResponse::error`.
pub async fn dispatch(
    method: &str,
    params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    match method {
        // Liveness probe.
        "ping" => handle_ping(params, state).await,

        // Backup domain.
        "run_backup" => handle_run_backup(params, state, event_tx).await,
        "get_backup_status" => handle_get_backup_status(params, state).await,
        "get_activity_series" => handle_get_activity_series(params, state).await,

        // Config domain.
        "get_config" => handle_get_config(params, state).await,
        "save_config" => handle_save_config(params, state, event_tx).await,
        "test_connection" => handle_test_connection(params, state).await,
        "get_system_info" => handle_get_system_info(params, state).await,
        "resolve_shell_bootstrap" => handle_resolve_shell_bootstrap(params, state).await,

        // Project domain.
        "list_projects" => handle_list_projects(params, state).await,

        // Snapshot domain.
        "list_snapshots" => handle_list_snapshots(params, state).await,
        "list_files" => handle_list_files(params, state).await,
        "read_snapshot_file" => handle_read_snapshot_file(params, state).await,
        "restore_snapshot" => handle_restore_snapshot(params, state, event_tx).await,
        "restore_all_snapshots" => handle_restore_all_snapshots(params, state, event_tx).await,
        "restore_all_projects" => handle_restore_all_projects(params, state, event_tx).await,

        // Pairing domain.
        "start_pairing" => handle_start_pairing(params, state).await,
        "stop_pairing" => handle_stop_pairing(params, state).await,
        "pairing_status" => handle_pairing_status(params, state).await,
        "discover_hosts" => handle_discover_hosts(params, state).await,
        "pair_with_host" => handle_pair_with_host(params, state).await,
        "confirm_pairing" => handle_confirm_pairing(params, state).await,

        // Host domain.
        "host_list_snapshot_projects" => handle_host_list_snapshot_projects(params, state).await,
        "host_volume_summary" => handle_host_volume_summary(params, state).await,
        "host_disk_inventory" => handle_host_disk_inventory(params, state).await,
        "host_trust_status" => handle_host_trust_status(params, state).await,
        "host_append_authorized_pubkey" => {
            handle_host_append_authorized_pubkey(params, state).await
        }
        "host_list_authorized_pubkeys" => handle_host_list_authorized_pubkeys(params, state).await,
        "host_remove_authorized_pubkey" => {
            handle_host_remove_authorized_pubkey(params, state).await
        }

        _ => Err(IpcError::new(
            "MethodNotFound",
            format!("unknown method: {method}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Liveness
// ---------------------------------------------------------------------------

/// Responds to a liveness probe with `{"pong": true}`.
///
/// Clients can use this to verify the daemon is running before sending heavier requests.
///
/// # Parameters
/// - `_params` — ignored.
/// - `_state`  — ignored.
async fn handle_ping(_params: Value, _state: Arc<DaemonState>) -> Result<Value, IpcError> {
    Ok(serde_json::json!({ "pong": true }))
}

// ---------------------------------------------------------------------------
// Backup domain
// ---------------------------------------------------------------------------

/// Triggers an async backup job (fire-and-forget). Returns immediately after scheduling.
///
/// Guards with an `AtomicBool` compare-exchange to reject concurrent runs.
/// Progress lines are broadcast via `IpcBroadcastSink`.
///
/// # Parameters
/// - `params`   — `{ "project": "<name>" }` (optional).
/// - `state`    — checked for `in_progress`; config read; `last_backup_at` updated.
/// - `event_tx` — broadcast sender used inside the spawned task for progress events.
async fn handle_run_backup(
    params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    let project: Option<String> = params
        .get("project")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Validate the optional project name before dispatching — rejects traversal attempts.
    if let Some(ref p) = project {
        validate_remote_component(p)
            .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    }

    // Reject if another backup is already running.
    if state
        .in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(cmd_err(BackrCommandError::backup_in_progress()));
    }

    let state2 = Arc::clone(&state);
    let tx = event_tx.clone();
    tokio::spawn(async move {
        // Ensure in_progress is cleared when the task exits, even on error.
        struct InProgressDrop(Arc<DaemonState>);
        impl Drop for InProgressDrop {
            fn drop(&mut self) {
                self.0.in_progress.store(false, Ordering::SeqCst);
            }
        }
        let _clear = InProgressDrop(Arc::clone(&state2));

        let sink: SharedProgress = Arc::new(IpcBroadcastSink::new(tx.clone()));
        let res = execute_backup_cycle_with_sink(sink, &state2, project).await;
        if let Err(err) = res {
            let _ = tx.send(IpcEvent {
                event: "backup_progress".into(),
                data: serde_json::json!(format!("[backr] error: {err}")),
            });
        }
        drop(_clear);
        {
            let mut ap = state2.active_project.lock().await;
            *ap = None;
        }
    });

    Ok(serde_json::json!(null))
}

/// Runs the full backup pipeline for one optional project (or all projects).
///
/// This is the engine used by both `handle_run_backup` (on-demand) and
/// `DaemonBackupTrigger::trigger_backup` (scheduled).  Mirrors the logic from
/// `src-tauri/src/commands/backup_cmd.rs::execute_backup_cycle_with_sink`.
///
/// # Inputs
///
/// * `sink`    — progress event receiver that broadcasts rsync lines to GUI clients.
/// * `state`   — shared daemon state (config, in-progress flag, active project).
/// * `project` — optional directory name restricting the backup to a single project.
pub(crate) async fn execute_backup_cycle_with_sink(
    sink: SharedProgress,
    state: &Arc<DaemonState>,
    project: Option<String>,
) -> Result<(), BackrError> {
    let cfg = {
        let g = state.config.lock().await;
        g.clone()
            .ok_or_else(|| BackrError::Config("daemon is not configured".into()))?
    };

    /* config::known_hosts_path resolves the isolated SSH known_hosts file used for all SSH calls. */
    let known_hosts = config::known_hosts_path()?;
    let projects = resolve_backup_targets(&cfg, project.as_deref())?;
    if projects.is_empty() {
        sink.backup_progress_line("[backr] no local projects found for backup".into());
        return Ok(());
    }

    for project_name in projects {
        {
            let mut ap = state.active_project.lock().await;
            *ap = Some(project_name.clone());
        }

        let local_dir = Path::new(&cfg.local.projects_path).join(&project_name);
        if !local_dir.exists() {
            return Err(BackrError::Msg(format!(
                "local project path missing: {}",
                local_dir.display()
            )));
        }

        /* ssh::remote_list_snapshot_names lists existing snapshots to compute the rsync link-dest. */
        let snapshots = ssh::remote_list_snapshot_names(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &cfg.remote.backup_path,
            &project_name,
        )
        .await
        .unwrap_or_default();

        let project_remote = ssh::remote_project_dir(&cfg.remote.backup_path, &project_name);
        /* ssh::ensure_remote_dir_exists creates the remote project directory if absent. */
        ssh::ensure_remote_dir_exists(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &project_remote,
        )
        .await?;

        let link_dest = snapshots.first().map(|name| {
            rsync::absolute_remote_snapshot_path(&cfg.remote.backup_path, &project_name, name)
        });

        let new_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let dest_folder =
            rsync::remote_snapshot_dest_folder(&cfg.remote.backup_path, &project_name, &new_name);

        sink.backup_progress_line(format!(
            "[backr] syncing project {project_name} -> {}@{}:{}",
            cfg.remote.user, cfg.remote.host, dest_folder
        ));

        /* rsync::rsync_backup_snapshot runs rsync with `--link-dest` incremental snapshot semantics. */
        rsync::rsync_backup_snapshot(
            sink.clone(),
            &cfg.remote.ssh_key,
            &known_hosts,
            &local_dir,
            link_dest.as_deref(),
            &cfg.remote.user,
            &cfg.remote.host,
            cfg.remote.port,
            &dest_folder,
        )
        .await?;

        let snapshot_count_after = snapshots.len() + 1;
        /* project_snapshot_cache::record_backup_success persists snapshot metadata to disk cache. */
        project_snapshot_cache::record_backup_success(
            &cfg,
            &project_name,
            &new_name,
            snapshot_count_after,
        )?;
    }

    let now = Utc::now();
    {
        let mut last = state.last_backup_at.lock().await;
        *last = Some(now);
    }

    let mut updated = cfg.clone();
    updated.state.last_backup_at = Some(now);
    /* config::save_config atomically writes the updated config to disk. */
    config::save_config(&updated)?;

    {
        let mut guard = state.config.lock().await;
        *guard = Some(updated);
    }

    sink.backup_progress_line("[backr] backup completed successfully".into());
    Ok(())
}

/// Resolves either every child directory of the configured projects root or one explicit project.
///
/// # Inputs
///
/// * `cfg`  — loaded configuration providing `local.projects_path`.
/// * `only` — optional folder name under `local.projects_path`.
///
/// # Returns
///
/// Sorted project directory names that exist on disk.
fn resolve_backup_targets(cfg: &Config, only: Option<&str>) -> Result<Vec<String>, BackrError> {
    let base = Path::new(&cfg.local.projects_path);
    if let Some(one) = only {
        let p = base.join(one);
        if !p.is_dir() {
            return Err(BackrError::Msg(format!(
                "project folder does not exist: {}",
                p.display()
            )));
        }
        return Ok(vec![one.to_string()]);
    }
    let mut out: Vec<String> = std::fs::read_dir(base)
        .map_err(BackrError::Io)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    Ok(out)
}

/// Returns current backup status: in-progress flag, timestamps, and active project.
///
/// Mirrors `src-tauri/src/commands/project_cmd.rs::get_backup_status`.
///
/// # Parameters
/// - `_params` — ignored.
/// - `state`   — provides `config`, `in_progress`, `last_backup_at`, `active_project`.
async fn handle_get_backup_status(
    _params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let cfg = state.require_config().await.map_err(cmd_err)?;

    let last_from_state = *state.last_backup_at.lock().await;
    let last_from_file = cfg.state.last_backup_at;
    let last_backup_at = last_from_state.or(last_from_file);

    let next_backup_at = match last_backup_at {
        Some(last) => Some(last + chrono::Duration::hours(cfg.schedule.interval_hours as i64)),
        None => Some(Utc::now() + chrono::Duration::hours(cfg.schedule.interval_hours as i64)),
    };

    to_json(serde_json::json!({
        "last_backup_at": last_backup_at,
        "next_backup_at": next_backup_at,
        "in_progress": state.in_progress.load(Ordering::SeqCst),
        "active_project": *state.active_project.lock().await,
    }))
}

/// Returns recent backup completion markers derived from persisted `[state]`.
///
/// Mirrors `src-tauri/src/commands/activity_cmd.rs::get_activity_series`.
///
/// # Parameters
/// - `_params` — ignored.
/// - `state`   — provides `config` for `state.last_backup_at`.
async fn handle_get_activity_series(
    _params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let cfg_opt = state.config.lock().await.clone();
    let Some(cfg) = cfg_opt else {
        return to_json(Vec::<Value>::new());
    };
    let mut pts: Vec<Value> = Vec::new();
    if let Some(last) = cfg.state.last_backup_at {
        pts.push(serde_json::json!({
            "ts_unix": last.timestamp(),
            "label": "backup_complete",
        }));
    }
    to_json(pts)
}

// ---------------------------------------------------------------------------
// Config domain
// ---------------------------------------------------------------------------

/// Returns the persisted configuration, or `null` when `config.toml` is absent.
///
/// Mirrors `src-tauri/src/commands/config_cmd.rs::get_config`.
///
/// # Parameters
/// - `_params` — ignored.
/// - `state`   — provides the in-memory `config` lock.
async fn handle_get_config(_params: Value, state: Arc<DaemonState>) -> Result<Value, IpcError> {
    let guard = state.config.lock().await;
    to_json(guard.clone())
}

/// Saves config to disk, updates `DaemonState`, and restarts the scheduler.
///
/// Mirrors `src-tauri/src/commands/config_cmd.rs::save_config`.
///
/// # Parameters
/// - `params`   — `{ "config": <Config> }` (the proxy wraps the Config under `config`).
/// - `state`    — config lock updated in place.
/// - `event_tx` — passed to the scheduler trigger so backup tasks can emit events.
async fn handle_save_config(
    params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    let config_val = params
        .get("config")
        .cloned()
        .ok_or_else(|| IpcError::new("InvalidInput", "save_config: missing 'config' parameter"))?;
    let next: Config = serde_json::from_value(config_val)
        .map_err(|e| IpcError::new("InvalidInput", e.to_string()))?;

    /* config::save_config writes config.toml atomically (write temp + rename). */
    config::save_config(&next).map_err(|e| cmd_err(BackrCommandError::from(e)))?;
    {
        let mut guard = state.config.lock().await;
        *guard = Some(next);
    }

    // Restart the scheduler so the new interval takes effect immediately.
    let cfg_snapshot = { state.config.lock().await.clone() };
    let sched = Arc::clone(&state.scheduler);
    let trigger: Arc<crate::scheduler::DaemonBackupTrigger> =
        Arc::new(crate::scheduler::DaemonBackupTrigger::new(
            Arc::clone(&state),
            event_tx,
        ));
    if let Err(e) = restart_scheduler(cfg_snapshot.as_ref(), &sched, trigger).await {
        tracing::warn!("save_config: failed to restart scheduler: {e}");
    }

    Ok(serde_json::json!(null))
}

/// Verifies SSH key-based authentication using a lightweight remote `echo` probe.
///
/// Mirrors `src-tauri/src/commands/config_cmd.rs::test_connection`.
///
/// # Parameters
/// - `params` — `{ "host", "user", "key_path", "ssh_port" }`.
async fn handle_test_connection(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let host = str_param(&params, "host")?;
    let user = str_param(&params, "user")?;
    let key_path = str_param(&params, "key_path")?;
    let ssh_port: u16 = params
        .get("ssh_port")
        .and_then(|v| v.as_u64())
        .map(|n| n as u16)
        .unwrap_or(22);

    /* config::expand_path_str resolves `~` and env vars in the key path. */
    let expanded = config::expand_path_str(&key_path)
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    /* ssh::test_connection runs a remote `echo` to verify key-based auth; returns BackrError. */
    backr_core::backup::ssh::test_connection(&host, &user, &expanded, ssh_port)
        .await
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    Ok(serde_json::json!(null))
}

/// Collects hostname, OS, kernel, arch, user, and a sample wall-clock instant.
///
/// Mirrors `src-tauri/src/commands/system_cmd.rs::get_system_info`.
///
/// # Parameters
/// - `_params` — ignored.
/// - `_state`  — ignored.
async fn handle_get_system_info(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let os_pretty = read_os_release_pretty().unwrap_or_else(|| {
        format!("{} ({})", std::env::consts::OS, std::env::consts::FAMILY)
    });

    to_json(serde_json::json!({
        "hostname": hostname_via_bin(),
        "os_pretty": os_pretty,
        "kernel_release": kernel_via_uname(),
        "arch": std::env::consts::ARCH,
        "user": std::env::var("USER").ok().or_else(|| std::env::var("USERNAME").ok()),
        "sampled_at_rfc3339": chrono::Local::now().to_rfc3339(),
    }))
}

/// Determines whether to send the user to setup, client, or host-dashboard mode.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::resolve_shell_bootstrap` (full implementation).
///
/// # Parameters
/// - `_params` — ignored.
/// - `_state`  — ignored (config loaded fresh from disk for accuracy).
async fn handle_resolve_shell_bootstrap(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    /* read_host_dashboard_marker reads /etc/backr/host.toml if this machine is configured as a NAS. */
    if let Some(marker) = read_host_dashboard_marker() {
        let root = Path::new(&marker.backup_root);
        if root.is_dir() {
            return to_json(serde_json::json!({
                "mode": "host",
                "backup_root": marker.backup_root,
                "ssh_user": marker.ssh_user,
            }));
        }
        tracing::warn!(
            "host_dashboard marker present but backup_root is not a directory: {}",
            marker.backup_root
        );
    }

    /* config::load_config reads config.toml from the user config directory if present. */
    match config::load_config() {
        Ok(Some(_)) => to_json(serde_json::json!({ "mode": "client" })),
        Ok(None) => to_json(serde_json::json!({ "mode": "setup" })),
        Err(err) => {
            tracing::warn!("resolve_shell_bootstrap: load_config failed: {err}");
            to_json(serde_json::json!({ "mode": "setup" }))
        }
    }
}

// ---------------------------------------------------------------------------
// Project domain
// ---------------------------------------------------------------------------

/// Lists immediate child directories of `local.projects_path` with snapshot stats.
///
/// Mirrors `src-tauri/src/commands/project_cmd.rs::list_projects`.
///
/// # Parameters
/// - `params` — `{ "probe_remote": bool }` (optional, defaults to false).
/// - `state`  — provides `config`.
async fn handle_list_projects(
    params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let probe_remote = params
        .get("probe_remote")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let base = Path::new(&cfg.local.projects_path);
    if !base.exists() {
        return Err(cmd_err(BackrCommandError::io(format!(
            "projects path does not exist: {}",
            cfg.local.projects_path
        ))));
    }

    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let key = remote_cache_key(&cfg);
    let disk_cache = load_snapshot_cache();
    let mut persisted = if disk_cache.remote_key == key {
        disk_cache.clone()
    } else {
        project_snapshot_cache::SnapshotCacheFile {
            remote_key: key.clone(),
            updated_at: None,
            projects: std::collections::HashMap::new(),
        }
    };
    persisted.remote_key = key.clone();

    let mut out: Vec<Value> = Vec::new();

    if !probe_remote {
        for name in names {
            match persisted.projects.get(&name) {
                Some(c) => {
                    out.push(serde_json::json!({
                        "name": name,
                        "last_backup_at": c.last_backup_at,
                        "snapshot_count": c.snapshot_count,
                        "stats_from_cache": true,
                    }));
                }
                None => {
                    out.push(serde_json::json!({
                        "name": name,
                        "last_backup_at": null,
                        "snapshot_count": 0,
                        "stats_from_cache": false,
                    }));
                }
            }
        }
        return to_json(out);
    }

    /* config::known_hosts_path resolves the isolated SSH known_hosts file path. */
    let known_hosts = config::known_hosts_path()
        .map_err(|e: BackrError| cmd_err(BackrCommandError::from(e)))?;

    for name in names {
        /* ssh::remote_list_snapshot_names lists snapshots via SSH; returns BackrError on failure. */
        match ssh::remote_list_snapshot_names(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &cfg.remote.backup_path,
            &name,
        )
        .await
        {
            Ok(snaps) => {
                let last = snaps
                    .first()
                    .and_then(|s| project_snapshot_cache::parse_snapshot_timestamp(s));
                persisted.projects.insert(
                    name.clone(),
                    project_snapshot_cache::CachedProjectStats {
                        last_backup_at: last,
                        snapshot_count: snaps.len(),
                    },
                );
                out.push(serde_json::json!({
                    "name": name,
                    "last_backup_at": last,
                    "snapshot_count": snaps.len(),
                    "stats_from_cache": false,
                }));
            }
            Err(_) => match persisted.projects.get(&name) {
                Some(c) => {
                    out.push(serde_json::json!({
                        "name": name,
                        "last_backup_at": c.last_backup_at,
                        "snapshot_count": c.snapshot_count,
                        "stats_from_cache": true,
                    }));
                }
                None => {
                    out.push(serde_json::json!({
                        "name": name,
                        "last_backup_at": null,
                        "snapshot_count": 0,
                        "stats_from_cache": false,
                    }));
                }
            },
        }
    }

    persisted.updated_at = Some(Utc::now());
    if let Err(err) = save_snapshot_cache(&persisted) {
        tracing::warn!("failed to persist project snapshot cache: {err}");
    }

    to_json(out)
}

// ---------------------------------------------------------------------------
// Snapshot domain
// ---------------------------------------------------------------------------

/// Lists snapshot folders for a project on the remote host, newest first.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::list_snapshots`.
///
/// # Parameters
/// - `params` — `{ "project": "<name>" }`.
async fn handle_list_snapshots(
    params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let project = str_param(&params, "project")?;
    validate_remote_component(&project)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    /* ssh::remote_list_snapshot_names lists snapshot directories on the remote host via SSH. */
    let names = ssh::remote_list_snapshot_names(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
    )
    .await
    .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let out: Vec<Value> = names
        .into_iter()
        .filter(|n| ssh::is_valid_snapshot_name(n))
        .map(|name| serde_json::json!({ "name": name }))
        .collect();

    to_json(out)
}

/// Lists immediate children for a path inside a snapshot using remote `find -maxdepth 1`.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::list_files`.
///
/// # Parameters
/// - `params` — `{ "project", "snapshot", "path" }`.
async fn handle_list_files(params: Value, state: Arc<DaemonState>) -> Result<Value, IpcError> {
    let project = str_param(&params, "project")?;
    let snapshot = str_param(&params, "snapshot")?;
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    validate_remote_component(&project)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    validate_remote_component(&snapshot)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    if !path.is_empty() && path != "." {
        validate_relative_path(path.trim().trim_start_matches('/'))
            .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    }

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let normalized = if path.is_empty() || path == "." {
        String::new()
    } else {
        path.trim().trim_start_matches('/').to_string()
    };

    /* ssh::remote_list_children runs a remote `find -maxdepth 1` inside the snapshot directory. */
    let rows = ssh::remote_list_children(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
        &snapshot,
        &normalized,
    )
    .await
    .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let out: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let is_dir = row.file_type == 'd';
            serde_json::json!({
                "name": row.name,
                "is_dir": is_dir,
                "size": row.size,
                "modified_unix": row.mtime_unix,
            })
        })
        .collect();

    to_json(out)
}

/// Reads a UTF-8 text slice from a snapshot file via remote `head -c`.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::read_snapshot_file`.
///
/// # Parameters
/// - `params` — `{ "project", "snapshot", "relative_path" }`.
async fn handle_read_snapshot_file(
    params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    const SNAPSHOT_READ_MAX_BYTES: u64 = 512 * 1024;

    let project = str_param(&params, "project")?;
    let snapshot = str_param(&params, "snapshot")?;
    let relative_path = str_param(&params, "relative_path")?;

    validate_remote_component(&project)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    validate_remote_component(&snapshot)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    if !ssh::is_valid_snapshot_name(&snapshot) {
        return Err(cmd_err(BackrCommandError::invalid_input(
            "invalid snapshot folder name",
        )));
    }

    let normalized = relative_path.trim().trim_start_matches('/').to_string();
    validate_relative_path(&normalized)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    /* ssh::remote_read_file_bytes fetches up to `max_bytes` from a remote file via SSH. */
    let bytes = ssh::remote_read_file_bytes(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
        &snapshot,
        &normalized,
        SNAPSHOT_READ_MAX_BYTES,
    )
    .await
    .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let truncated = bytes.len() as u64 >= SNAPSHOT_READ_MAX_BYTES;
    let text = String::from_utf8(bytes).map_err(|_| {
        cmd_err(BackrCommandError::invalid_input(
            "file is not valid UTF-8 (binary files cannot be previewed)",
        ))
    })?;

    to_json(serde_json::json!({ "text": text, "truncated": truncated }))
}

/// Restores an entire snapshot under home, using `IpcBroadcastSink` for progress.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::restore_snapshot`.
///
/// # Parameters
/// - `params`   — `{ "project", "snapshot" }`.
/// - `event_tx` — used to construct `IpcBroadcastSink` for rsync progress events.
async fn handle_restore_snapshot(
    params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    let project = str_param(&params, "project")?;
    let snapshot = str_param(&params, "snapshot")?;

    validate_remote_component(&project)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    validate_remote_component(&snapshot)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let sink: SharedProgress = Arc::new(IpcBroadcastSink::new(event_tx));
    /* restore_single_snapshot_to_home runs rsync to restore one snapshot locally. */
    let dest = restore_single_snapshot_to_home(sink, &cfg, &known, &project, &snapshot)
        .await
        .map_err(core_err)?;

    to_json(dest)
}

/// Restores every indexed snapshot for `project` sequentially.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::restore_all_snapshots`.
///
/// # Parameters
/// - `params`   — `{ "project" }`.
/// - `event_tx` — used to construct `IpcBroadcastSink` for rsync progress events.
async fn handle_restore_all_snapshots(
    params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    let project = str_param(&params, "project")?;
    validate_remote_component(&project)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;

    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    let sink: SharedProgress = Arc::new(IpcBroadcastSink::new(event_tx));
    /* restore_every_valid_snapshot_for_project iterates all valid snapshots and restores each. */
    let paths =
        restore_every_valid_snapshot_for_project(sink, &cfg, &known, &project)
            .await
            .map_err(core_err)?;

    to_json(paths)
}

/// Restores all valid snapshots for every immediate child project directory.
///
/// Mirrors `src-tauri/src/commands/snapshot_cmd.rs::restore_all_projects`.
///
/// # Parameters
/// - `_params`  — ignored.
/// - `event_tx` — used to construct `IpcBroadcastSink` for rsync progress events.
async fn handle_restore_all_projects(
    _params: Value,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) -> Result<Value, IpcError> {
    let cfg = state.require_config().await.map_err(cmd_err)?;
    let known = config::known_hosts_path().map_err(|e| cmd_err(BackrCommandError::from(e)))?;
    let base = Path::new(&cfg.local.projects_path);

    /* local_child_directory_names enumerates subdirectories of the local projects root. */
    let projects = local_child_directory_names(base).map_err(core_err)?;

    let mut rows: Vec<Value> = Vec::new();
    for project in projects {
        let sink: SharedProgress = Arc::new(IpcBroadcastSink::new(event_tx.clone()));
        /* restore_every_valid_snapshot_for_project restores all snapshots for a single project. */
        let destinations =
            restore_every_valid_snapshot_for_project(sink, &cfg, &known, &project)
                .await
                .map_err(core_err)?;
        if !destinations.is_empty() {
            rows.push(serde_json::json!({
                "project": project,
                "destinations": destinations,
            }));
        }
    }

    to_json(rows)
}

// ---------------------------------------------------------------------------
// Snapshot restore helpers
// ---------------------------------------------------------------------------

/// Runs one rsync restore for `snapshot`; shared by single-restore and bulk-restore handlers.
///
/// # Inputs
///
/// * `sink`        — progress event receiver (rsync lines broadcast to GUI clients).
/// * `cfg`         — loaded configuration (SSH key, host, user, port, backup path).
/// * `known_hosts` — path to the isolated SSH known_hosts file.
/// * `project`     — project directory name under the backup root.
/// * `snapshot`    — snapshot directory name (validated timestamp format).
///
/// # Returns
///
/// Absolute local destination path where files were restored.
async fn restore_single_snapshot_to_home(
    sink: SharedProgress,
    cfg: &Config,
    known_hosts: &Path,
    project: &str,
    snapshot: &str,
) -> Result<String, BackrError> {
    let remote_dir =
        rsync::absolute_remote_snapshot_path(&cfg.remote.backup_path, project, snapshot);
    let remote_url = format!(
        "{}@{}:{}/",
        cfg.remote.user, cfg.remote.host, remote_dir
    );

    let home = dirs::home_dir()
        .ok_or_else(|| BackrError::Msg("could not resolve home directory".into()))?;
    let base_local = home.join(restore_destination_folder_base(snapshot));
    let destination = uniquify_path(&base_local)?;

    std::fs::create_dir_all(&destination).map_err(BackrError::Io)?;

    /* rsync::rsync_restore_snapshot runs an rsync pull from remote snapshot to local destination. */
    rsync::rsync_restore_snapshot(
        sink,
        &cfg.remote.ssh_key,
        known_hosts,
        &remote_url,
        &cfg.remote.host,
        cfg.remote.port,
        &destination,
    )
    .await?;

    Ok(destination.to_string_lossy().into_owned())
}

/// Restores every strictly-named remote snapshot for `project`, newest first.
///
/// # Inputs
///
/// * `sink`        — progress event receiver broadcast to GUI clients.
/// * `cfg`         — loaded configuration.
/// * `known_hosts` — path to the isolated SSH known_hosts file.
/// * `project`     — project directory name under the backup root.
async fn restore_every_valid_snapshot_for_project(
    sink: SharedProgress,
    cfg: &Config,
    known_hosts: &Path,
    project: &str,
) -> Result<Vec<String>, BackrError> {
    /* ssh::remote_list_snapshot_names lists all snapshot directory names via SSH for the project. */
    let names = ssh::remote_list_snapshot_names(
        &cfg.remote.ssh_key,
        known_hosts,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        project,
    )
    .await?;

    let mut snapshots: Vec<String> = names
        .into_iter()
        .filter(|n| ssh::is_valid_snapshot_name(n))
        .collect();
    snapshots.sort_by(|a, b| b.cmp(a));

    let mut paths = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        paths.push(
            restore_single_snapshot_to_home(
                sink.clone(),
                cfg,
                known_hosts,
                project,
                &snapshot,
            )
            .await?,
        );
    }
    Ok(paths)
}

/// Builds the restore destination folder base name from a snapshot identifier.
///
/// Standard snapshot IDs (`YYYY-MM-DD_HH-MM-SS`) embed time already → `Projects-<id>`.
/// Non-standard names append the current UTC stamp so restores stay traceable.
fn restore_destination_folder_base(snapshot: &str) -> String {
    if ssh::is_valid_snapshot_name(snapshot) {
        format!("Projects-{snapshot}")
    } else {
        let stamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
        let mut safe: String = snapshot
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(120)
            .collect();
        while safe.ends_with('_') {
            safe.pop();
        }
        let suffix = if safe.is_empty() {
            "snapshot".to_string()
        } else {
            safe
        };
        format!("Projects-{suffix}-{stamp}")
    }
}

/// Returns sorted directory basenames under `base`.
///
/// # Inputs
///
/// * `base` — `local.projects_path`; must exist as a directory.
fn local_child_directory_names(base: &Path) -> Result<Vec<String>, BackrError> {
    if !base.exists() {
        return Err(BackrError::Msg(format!(
            "projects path does not exist: {}",
            base.display()
        )));
    }
    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(BackrError::Io)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// Appends `-1`, `-2`, ... when `path` already exists on disk.
///
/// # Inputs
///
/// * `path` — desired destination directory.
///
/// # Returns
///
/// A non-existing directory path adjacent to `path`.
fn uniquify_path(path: &Path) -> Result<std::path::PathBuf, BackrError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let parent = path
        .parent()
        .ok_or_else(|| BackrError::Msg("invalid restore parent".into()))?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| BackrError::Msg("invalid restore path".into()))?;
    for i in 1..10_000u32 {
        let candidate = parent.join(format!("{file_name}-{i}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(BackrError::Msg(
        "could not find a free restore folder suffix".into(),
    ))
}

// ---------------------------------------------------------------------------
// Pairing domain
// ---------------------------------------------------------------------------

/// Opens a pairing window (code + mDNS advertise + listener) on the host side.
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::start_pairing`.
/// Auto-tears-down after `PAIRING_TTL_SECS` seconds.
///
/// # Parameters
/// - `_params` — ignored.
/// - `state`   — pairing session and pairing_runtime slots updated.
async fn handle_start_pairing(
    _params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    // Tear down any prior pairing window before opening a new one.
    stop_pairing_internal(&state).await;

    /* gather_host_info collects hostname, SSH pubkey, and port for the pairing payload. */
    let host =
        gather_host_info().map_err(|e| cmd_err(BackrCommandError::pairing(e)))?;
    let session = PairingSession::new();
    let code = session.code().to_string();

    /* Server::http binds an ephemeral TCP listener for the pairing HTTP handshake. */
    let server = Server::http("0.0.0.0:0")
        .map_err(|e| cmd_err(BackrCommandError::pairing(e.to_string())))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or_else(|| {
            cmd_err(BackrCommandError::pairing(
                "could not resolve pairing listener port",
            ))
        })?;
    let server = Arc::new(server);

    /* advertise broadcasts this host over mDNS on the resolved port. */
    let (mdns, fullname) =
        advertise(port).map_err(|e| cmd_err(BackrCommandError::pairing(e)))?;

    *state.pairing.lock().await = Some(session);

    /* serve runs the pairing HTTP handler on a dedicated OS thread (blocking I/O). */
    let serve_state: Arc<dyn backr_core::pairing::listener::PairingStateAccess> =
        Arc::clone(&state) as _;
    let serve_server = server.clone();
    let serve_host = host.clone();
    let handle = thread::spawn(move || serve(serve_server, serve_state, serve_host));

    *state.pairing_runtime.lock().await = Some(PairingRuntime {
        mdns,
        fullname,
        server,
        thread: Some(handle),
    });

    // TTL task: auto-close the pairing window after PAIRING_TTL_SECS seconds.
    let ttl_state = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(PAIRING_TTL_SECS)).await;
        if ttl_state.pairing_runtime.lock().await.is_some() {
            tracing::info!("pairing TTL expired — closing window");
            stop_pairing_internal(&ttl_state).await;
        }
    });

    // Return the host's own SSH key fingerprint alongside the code so the host UI
    // can display it for out-of-band verification: the laptop shows the same
    // fingerprint and asks the user to confirm the two screens match. Without this
    // the host showed only the code and silently trusted the laptop on code-match,
    // leaving the laptop's "verify against the host screen" step impossible.
    to_json(serde_json::json!({
        "code": code,
        "host_key_fingerprint": host.host_key_fingerprint,
    }))
}

/// Closes the pairing window if one is open.
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::stop_pairing`.
///
/// # Parameters
/// - `_params` — ignored.
async fn handle_stop_pairing(
    _params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    stop_pairing_internal(&state).await;
    Ok(serde_json::json!(null))
}

/// Returns `true` while a pairing window is open.
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::pairing_status`.
///
/// # Parameters
/// - `_params` — ignored.
async fn handle_pairing_status(
    _params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let open = state.pairing_runtime.lock().await.is_some();
    to_json(open)
}

/// Browses the LAN for hosts currently in pairing mode (~2.5s window).
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::discover_hosts`.
///
/// # Parameters
/// - `_params` — ignored.
async fn handle_discover_hosts(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    /* tokio::task::spawn_blocking runs the mDNS browse on a thread-pool thread (blocking call). */
    let hosts: Vec<DiscoveredHost> =
        tokio::task::spawn_blocking(|| do_discover_hosts(Duration::from_millis(2500)))
            .await
            .map_err(|e| cmd_err(BackrCommandError::task_failed(e.to_string())))?
            .map_err(|e| cmd_err(BackrCommandError::pairing(e)))?;

    to_json(hosts)
}

/// Pairs this machine with a discovered host using the 6-digit code.
///
/// Returns a `PairDraft` containing the prefilled config and the host's SSH key
/// fingerprint for out-of-band verification before calling `confirm_pairing`.
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::pair_with_host`.
///
/// # Parameters
/// - `params` — `{ "address": "<ip:port>", "code": "<6-digit>" }`.
async fn handle_pair_with_host(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let address = str_param(&params, "address")?;
    let code = str_param(&params, "code")?;

    /* tokio::task::spawn_blocking runs the HTTP pairing exchange on a thread-pool thread. */
    let draft: PairDraft =
        tokio::task::spawn_blocking(move || do_pair_with_host(&address, &code))
            .await
            .map_err(|e| cmd_err(BackrCommandError::task_failed(e.to_string())))?
            .map_err(|e| cmd_err(BackrCommandError::pairing(e)))?;

    to_json(draft)
}

/// Finalizes a confirmed pair: pins the host's SSH key and returns the ready-to-save config.
///
/// Mirrors `src-tauri/src/commands/pairing_cmd.rs::confirm_pairing`.
///
/// # Parameters
/// - `params` — `{ "draft": <PairDraft> }` (the `PairDraft` returned by `pair_with_host`).
async fn handle_confirm_pairing(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let draft_val = params
        .get("draft")
        .cloned()
        .ok_or_else(|| IpcError::new("InvalidInput", "confirm_pairing: missing 'draft' parameter"))?;
    let draft: PairDraft = serde_json::from_value(draft_val)
        .map_err(|e| IpcError::new("InvalidInput", e.to_string()))?;

    /* confirm_pair_draft pins the host public key into known_hosts and returns the final Config. */
    let cfg: Config = tokio::task::spawn_blocking(move || confirm_pair_draft(draft))
        .await
        .map_err(|e| cmd_err(BackrCommandError::task_failed(e.to_string())))?
        .map_err(|e| cmd_err(BackrCommandError::pairing(e)))?;

    to_json(cfg)
}

/// Tears down any active pairing runtime and clears the pairing session.
///
/// # Inputs
///
/// * `state` — daemon state holding pairing session and runtime slots.
async fn stop_pairing_internal(state: &Arc<DaemonState>) {
    let rt = state.pairing_runtime.lock().await.take();
    if let Some(rt) = rt {
        /* PairingRuntime::shutdown stops the mDNS daemon and sends a shutdown signal to the HTTP server. */
        rt.shutdown();
    }
    *state.pairing.lock().await = None;
}

// ---------------------------------------------------------------------------
// Host domain
// ---------------------------------------------------------------------------

/// Lists projects by scanning `backup_root/<project>/<snapshot>/` locally on the NAS machine.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_list_snapshot_projects`.
///
/// # Parameters
/// - `params` — `{ "backup_root": "<path>" }`.
async fn handle_host_list_snapshot_projects(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let backup_root = str_param(&params, "backup_root")?;
    let base = Path::new(&backup_root);
    if !base.is_dir() {
        return Err(cmd_err(BackrCommandError::invalid_input(format!(
            "backup_root is not a directory: {}",
            backup_root
        ))));
    }

    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let mut out: Vec<Value> = Vec::new();
    for name in names {
        let project_path = base.join(&name);
        let snaps = snapshot_dirs(&project_path);
        let snapshot_count = snaps.len();
        let last_backup_at = snaps
            .first()
            .and_then(|s| project_snapshot_cache::parse_snapshot_timestamp(s));
        let recent_snapshots: Vec<&String> = snaps.iter().take(3).collect();
        out.push(serde_json::json!({
            "name": name,
            "snapshot_count": snapshot_count,
            "last_backup_at": last_backup_at,
            "recent_snapshots": recent_snapshots,
        }));
    }

    to_json(out)
}

/// Builds volume summary by probing `df` against `backup_root`.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_volume_summary`.
///
/// # Parameters
/// - `params` — `{ "backup_root": "<path>" }`.
async fn handle_host_volume_summary(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    use std::process::Command;

    let backup_root = str_param(&params, "backup_root")?;
    if !Path::new(&backup_root).exists() {
        return Err(cmd_err(BackrCommandError::invalid_input(format!(
            "backup_root does not exist: {}",
            backup_root
        ))));
    }

    // Try GNU df with full output first.
    let enriched = Command::new("df")
        .args(["-B1", "--output=source,target,avail,size,used,pcent", &backup_root])
        .output()
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    if enriched.status.success() {
        let text = String::from_utf8_lossy(&enriched.stdout);
        let mut lines = text.lines();
        lines.next(); // skip header
        if let Some(line) = lines.next() {
            if let Some((fs, mp, avail, size, used, pct)) = parse_df_full_row(line) {
                return to_json(serde_json::json!({
                    "backup_root": backup_root,
                    "bytes_avail": avail,
                    "bytes_size": size,
                    "filesystem_source": fs,
                    "mount_point": mp,
                    "used_bytes": used,
                    "used_percent": pct,
                }));
            }
        }
    }

    // Fallback to legacy two-column df.
    let legacy = Command::new("df")
        .args(["-B1", "--output=avail,size", &backup_root])
        .output()
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    if !legacy.status.success() {
        return to_json(serde_json::json!({
            "backup_root": backup_root,
            "bytes_avail": null,
            "bytes_size": null,
        }));
    }

    let text = String::from_utf8_lossy(&legacy.stdout);
    let mut lines = text.lines();
    lines.next(); // skip header
    let line = lines.next().unwrap_or("");
    let (avail, size) = parse_df_legacy_row(line);

    to_json(serde_json::json!({
        "backup_root": backup_root,
        "bytes_avail": avail,
        "bytes_size": size,
    }))
}

/// Runs `du`-backed disk inventory in a blocking thread.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_disk_inventory`.
///
/// # Parameters
/// - `params` — `{ "backup_root": "<path>", "force_refresh": bool }`.
async fn handle_host_disk_inventory(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let backup_root = str_param(&params, "backup_root")?;
    let force_refresh = params
        .get("force_refresh")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    /* tokio::task::spawn_blocking runs `du` on a thread-pool thread (blocking I/O). */
    let inventory =
        tokio::task::spawn_blocking(move || {
            host_disk_inventory::host_disk_inventory_impl(backup_root, force_refresh)
        })
        .await
        .map_err(|e| {
            cmd_err(BackrCommandError::task_failed(format!(
                "disk inventory task failed: {e}"
            )))
        })?
        .map_err(|e| cmd_err(BackrCommandError::from(e)))?;

    to_json(inventory)
}

/// Reports `authorized_keys` path and pubkey count for the host Trust page.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_trust_status`.
///
/// # Parameters
/// - `_params` — ignored.
async fn handle_host_trust_status(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    /* host_trust::host_trust_status_impl reads and parses authorized_keys on the host. */
    let status = host_trust::host_trust_status_impl()
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    to_json(status)
}

/// Appends one validated pubkey line to `authorized_keys`, or returns sudo fallback commands.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_append_authorized_pubkey`.
///
/// # Parameters
/// - `params` — `{ "pubkey_line": "<OpenSSH pubkey>" }`.
async fn handle_host_append_authorized_pubkey(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let pubkey_line = str_param(&params, "pubkey_line")?;
    /* host_trust::host_append_authorized_pubkey_impl validates and appends one pubkey line. */
    let result = host_trust::host_append_authorized_pubkey_impl(pubkey_line)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    to_json(result)
}

/// Lists every parsed pubkey entry in `authorized_keys`.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_list_authorized_pubkeys`.
///
/// # Parameters
/// - `_params` — ignored.
async fn handle_host_list_authorized_pubkeys(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    /* host_trust::host_list_authorized_pubkeys_impl reads and parses all authorized_keys entries. */
    let entries = host_trust::host_list_authorized_pubkeys_impl()
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    to_json(entries)
}

/// Removes one pubkey line (identified by exact `raw_line` match) from `authorized_keys`.
///
/// Mirrors `src-tauri/src/commands/host_cmd.rs::host_remove_authorized_pubkey`.
///
/// # Parameters
/// - `params` — `{ "raw_line": "<exact authorized_keys line>" }`.
async fn handle_host_remove_authorized_pubkey(
    params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    let raw_line = str_param(&params, "raw_line")?;
    /* host_trust::host_remove_authorized_pubkey_impl removes a pubkey line by exact match. */
    let result = host_trust::host_remove_authorized_pubkey_impl(raw_line)
        .map_err(|e| cmd_err(BackrCommandError::invalid_input(e)))?;
    to_json(result)
}

// ---------------------------------------------------------------------------
// Host domain helpers
// ---------------------------------------------------------------------------

/// Lists snapshot subdirectory names for `project_path`, newest valid snapshot names first.
///
/// # Inputs
///
/// * `project_path` — absolute path to a project directory under `backup_root`.
///
/// # Returns
///
/// Sorted snapshot names (newest first) that have parseable timestamps.
fn snapshot_dirs(project_path: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(project_path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| project_snapshot_cache::parse_snapshot_timestamp(n).is_some())
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    names
}

/// Parses GNU `df --output=source,target,avail,size,used,pcent` data row.
///
/// # Inputs
///
/// * `line` — non-header whitespace-separated row from `df -B1`.
///
/// # Returns
///
/// Tuple `(device, mount, avail_bytes, size_bytes, used_bytes, pct_string)` if all six columns parse.
fn parse_df_full_row(line: &str) -> Option<(String, String, u64, u64, u64, String)> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 6 {
        return None;
    }
    Some((
        cols[0].to_string(),
        cols[1].to_string(),
        cols[2].parse().ok()?,
        cols[3].parse().ok()?,
        cols[4].parse().ok()?,
        cols[5].to_string(),
    ))
}

/// Parses legacy two-column `df --output=avail,size` row.
///
/// # Returns
///
/// `(bytes_avail, bytes_size)` with `None` for missing or unparseable columns.
fn parse_df_legacy_row(line: &str) -> (Option<u64>, Option<u64>) {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 2 {
        return (None, None);
    }
    (cols[0].parse().ok(), cols[1].parse().ok())
}

// ---------------------------------------------------------------------------
// System info helpers
// ---------------------------------------------------------------------------

/// Parses `PRETTY_NAME="..."` from `/etc/os-release` when present.
///
/// # Returns
///
/// Trimmed quoted distro description, or `None` when the file or field is missing.
fn read_os_release_pretty() -> Option<String> {
    let data = std::fs::read_to_string("/etc/os-release").ok()?;
    for raw in data.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("PRETTY_NAME=") else {
            continue;
        };
        let v = rest.trim().trim_matches('"').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// Best-effort short hostname via the `hostname` executable (POSIX / Windows).
///
/// # Returns
///
/// Hostname string, or `None` when the command is unavailable or fails.
fn hostname_via_bin() -> Option<String> {
    let out = std::process::Command::new("hostname").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Kernel version token via `uname -r` on Unix-like hosts.
///
/// # Returns
///
/// Kernel release string, or `None` when `uname` is unavailable or fails.
fn kernel_via_uname() -> Option<String> {
    let out = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
