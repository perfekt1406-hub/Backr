/*
 * QEMU/KVM Debian guest integration — runs the real SSH/find/rsync backup pipeline from `backr_lib`
 * against a cloud-init Debian VM (ssh forwarded to localhost:2222 by the harness script).
 *
 * Run from repo root:
 *   ./scripts/backr-vm-debian-integration.sh
 *
 * Environment (exported by script): BACKR_VM_HOST BACKR_VM_PORT BACKR_VM_USER BACKR_VM_KEY_PATH
 */

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use backr_lib::backup::ssh::{remote_list_children, remote_list_snapshot_names};
use backr_lib::commands::backup_cmd::execute_backup_cycle_with_sink;
use backr_lib::config::{
    expand_path_str, known_hosts_path, Config, LocalConfig, RemoteConfig, ScheduleConfig,
    StateConfig, UpdateConfig, CONFIG_VERSION,
};
use backr_lib::progress_sink::CollectLines;
use backr_lib::state::AppState;

#[tokio::test]
#[ignore]
async fn qemu_debian_backup_pipeline_matches_remote_find() {
    let _guard = EnvGuard::isolate_xdg_home();

    let tmp_root = TempDir::new().expect("project temp dir");
    let demo = tmp_root.path().join("demo");
    fs::create_dir_all(&demo).expect("mkdir demo");
    fs::write(demo.join("ping.txt"), b"vm-integration").expect("write ping.txt");

    let host = std::env::var("BACKR_VM_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = std::env::var("BACKR_VM_PORT")
        .unwrap_or_else(|_| "2222".to_string())
        .parse()
        .expect("BACKR_VM_PORT numeric");
    let user = std::env::var("BACKR_VM_USER").unwrap_or_else(|_| "debian".to_string());
    let key_path = PathBuf::from(std::env::var("BACKR_VM_KEY_PATH").expect(
        "BACKR_VM_KEY_PATH (private key produced by scripts/backr-vm-debian-integration.sh)",
    ));

    let key_expanded = expand_path_str(&key_path.display().to_string()).expect("expand ssh key");

    let config = Config {
        version: CONFIG_VERSION,
        remote: RemoteConfig {
            host: host.clone(),
            user,
            ssh_key: key_expanded,
            port,
            backup_path: "/srv/backups".into(),
        },
        local: LocalConfig {
            projects_path: tmp_root.path().to_string_lossy().into_owned(),
        },
        schedule: ScheduleConfig { interval_hours: 24 },
        state: StateConfig {
            last_backup_at: None,
        },
        update: UpdateConfig::default(),
    };

    backr_lib::backup::ssh::test_connection(
        &config.remote.host,
        &config.remote.user,
        &config.remote.ssh_key,
        port,
    )
    .await
    .expect(
        "ssh hello failed — start the VM script or check hostfwd 2222, user \"debian\", and key path",
    );

    bootstrap_remote_rsync_via_ssh(
        &config.remote.host,
        &config.remote.user,
        port,
        &config.remote.ssh_key,
    )
    .await;

    let known = known_hosts_path().expect("resolved known_hosts");
    let xdgh = PathBuf::from(std::env::var("XDG_CONFIG_HOME").expect("guard sets XDG"));
    assert_eq!(
        known,
        xdgh.join("backr").join("known_hosts"),
        "isolates SSH host keys inside XDG_CONFIG_HOME/backr/"
    );

    let state = Arc::new(AppState::default());
    {
        let mut g = state.config.lock().await;
        *g = Some(config.clone());
    }

    let collect = CollectLines::default();
    let sink = collect.clone().into_shared();
    execute_backup_cycle_with_sink(sink, &state, None)
        .await
        .expect("backup cycle");

    let snaps = remote_list_snapshot_names(
        &config.remote.ssh_key,
        &known,
        &config.remote.host,
        &config.remote.user,
        port,
        "/srv/backups",
        "demo",
    )
    .await
    .expect("snapshot list");
    assert!(
        !snaps.is_empty(),
        "expected at least one dated snapshot under demo (reused VMs may have older runs too)"
    );
    let newest = &snaps[0];
    assert!(snap_name_format(newest), "snapshot {:?} naming", newest);

    let children = remote_list_children(
        &config.remote.ssh_key,
        &known,
        &config.remote.host,
        &config.remote.user,
        port,
        "/srv/backups",
        "demo",
        newest,
        "",
    )
    .await
    .expect("list snapshot root");
    assert!(
        children.iter().any(|e| e.name == "ping.txt"),
        "{children:?}"
    );

    eprintln!(
        "progress excerpts: {}",
        collect.lines.lock().unwrap().join(" | ")
    );
}

fn snap_name_format(s: &str) -> bool {
    backr_lib::backup::ssh::is_valid_snapshot_name(s)
}

/// Waits until cloud-init / unattended-upgrades release apt locks, then installs `rsync` if missing.
///
/// Debian generic cloud images typically omit `rsync`; Backr's backup shells out to local and remote `rsync`.
/// Uses `ssh` from PATH so the test matches production tooling.
///
/// # Inputs
///
/// * `host` / `user` — guest SSH target behind QEMU `hostfwd`.
/// * `port` — forwarded SSH port on the host (often `2222`).
/// * `key_path` — expanded private key path usable by `ssh -i`.
async fn bootstrap_remote_rsync_via_ssh(host: &str, user: &str, port: u16, key_path: &str) {
    let target = format!("{user}@{host}");
    let bootstrap = r#"n=0; while [ "$n" -lt 120 ] && ( sudo fuser /var/lib/apt/lists/lock >/dev/null 2>&1 || sudo fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 ); do n=$((n+1)); sleep 3; done; command -v rsync >/dev/null 2>&1 && exit 0; sudo env DEBIAN_FRONTEND=noninteractive apt-get update -qq && sudo env DEBIAN_FRONTEND=noninteractive apt-get install -qq -y rsync"#;
    let out = tokio::process::Command::new("ssh")
        .args([
            "-p",
            &port.to_string(),
            "-i",
            key_path,
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            &target,
            "bash",
            "-c",
            bootstrap,
        ])
        .output()
        .await
        .expect("spawn ssh for rsync bootstrap");
    if !out.status.success() {
        panic!(
            "remote rsync install failed code {:?} stderr: {} stdout: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

/// Restores `$XDG_CONFIG_HOME` after the test while keeping isolated config under a temp dir.
struct EnvGuard {
    previous: Option<String>,
    _tmpdir: tempfile::TempDir,
}

impl EnvGuard {
    fn isolate_xdg_home() -> Self {
        let tmpdir = tempfile::TempDir::new().expect("xdg temp");
        let previous = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", tmpdir.path());
        Self {
            previous,
            _tmpdir: tmpdir,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
