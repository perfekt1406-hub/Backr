/*
 * main.rs — Entry point for the `backr` CLI.
 *
 * Parses command-line arguments with `clap` (derive API), then dispatches each
 * subcommand to the appropriate IPC call against the `backrd` Unix socket.
 *
 * Subcommand → IPC method mapping:
 *
 *   backup [PROJECT]              → run_backup          (streams progress)
 *   status                        → get_backup_status
 *   list [PROJECT]                → list_projects | list_snapshots
 *   config get <KEY>              → get_config (then extracts the key)
 *   config set <KEY> <VALUE>      → stub (prints guidance)
 *   pair                          → stub (wizard not yet interactive)
 *   snapshots [PROJECT]           → list_snapshots
 *   trust add <PUBKEY>            → host_append_authorized_pubkey
 *   trust list                    → host_list_authorized_pubkeys
 *   trust remove <RAW_LINE>       → host_remove_authorized_pubkey
 */

mod client;
mod output;
mod update_worker;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::Value;

// ---------------------------------------------------------------------------
// CLI structure
// ---------------------------------------------------------------------------

/// Backr backup client — talks to the running `backrd` daemon.
#[derive(Parser, Debug)]
#[command(name = "backr", about = "Backr backup client", version)]
struct Cli {
    /// Print raw JSON response instead of formatted output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a backup now (optionally for a single project).
    Backup {
        /// Optional project directory name to back up.
        project: Option<String>,
    },

    /// Show current backup status (in-progress, last/next timestamps).
    Status,

    /// List projects (no PROJECT) or snapshots for a specific project.
    List {
        /// When provided, list snapshots for this project instead of all projects.
        project: Option<String>,
    },

    /// Get or set daemon configuration values.
    Config(ConfigArgs),

    /// Launch the interactive pairing wizard.
    Pair,

    /// List snapshots for a project.
    Snapshots {
        /// Project name to list snapshots for.
        project: Option<String>,
    },

    /// Manage trusted host public keys (host-only).
    Trust(TrustArgs),

    /// Download, verify, and apply the latest release (or just check with --check).
    Update {
        /// Only report whether an update is available; do not apply it.
        #[arg(long)]
        check: bool,
        /// Internal: launched by the daemon; suppresses interactive output.
        #[arg(long, hide = true)]
        from_daemon: bool,
    },

    /// Enable, disable, or show automatic updates.
    Autoupdate(AutoupdateArgs),
}

// ---------------------------------------------------------------------------
// Config subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `config` subcommand group.
#[derive(Args, Debug)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

/// Sub-actions under `config`.
#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Retrieve a top-level config key (e.g. `remote.host`).
    Get {
        /// Dot-separated config key path (e.g. `remote.host`, `schedule.interval_hours`).
        key: String,
    },
    /// Set a config value (not yet implemented — use the GUI).
    Set {
        /// Dot-separated config key path.
        key: String,
        /// New value (string form).
        value: String,
    },
}

// ---------------------------------------------------------------------------
// Trust subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `trust` subcommand group (host-only).
#[derive(Args, Debug)]
struct TrustArgs {
    #[command(subcommand)]
    action: TrustAction,
}

/// Sub-actions under `trust`.
#[derive(Subcommand, Debug)]
enum TrustAction {
    /// Add a public key to the trusted hosts list.
    Add {
        /// Full OpenSSH public key line (e.g. `ssh-ed25519 AAAA... comment`).
        pubkey: String,
    },
    /// List all currently trusted public keys.
    List,
    /// Remove a trusted key by exact raw authorized_keys line.
    Remove {
        /// The exact raw line from `authorized_keys` to remove.
        raw_line: String,
    },
}

// ---------------------------------------------------------------------------
// Autoupdate subcommand
// ---------------------------------------------------------------------------

/// Arguments for the `autoupdate` subcommand group.
#[derive(Args, Debug)]
struct AutoupdateArgs {
    #[command(subcommand)]
    action: AutoupdateAction,
}

/// Sub-actions under `autoupdate`.
#[derive(Subcommand, Debug)]
enum AutoupdateAction {
    /// Turn automatic updates on.
    On,
    /// Turn automatic updates off.
    Off,
    /// Show whether automatic updates are enabled.
    Status,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Tokio async runtime entry point.
///
/// Parses the CLI, dispatches to the appropriate IPC helper, and prints the result.
/// Exits with code 1 on any error, printing the message to stderr.
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Dispatches the parsed `Cli` to the correct IPC call and formats the response.
///
/// # Parameters
///
/// - `cli` — Fully parsed CLI struct from clap.
///
/// # Returns
///
/// `Ok(())` on success; `Err` with a human-readable message on failure.
async fn run(cli: Cli) -> Result<()> {
    // Auto-update tick: poke the daemon to consider applying an update (a no-op when
    // auto-update is off or the throttle window hasn't elapsed). Best-effort — never
    // blocks or fails the requested command. Skipped for the update commands themselves
    // so they don't trigger a competing update of their own.
    if !matches!(cli.command, Commands::Update { .. } | Commands::Autoupdate(_)) {
        let _ = client::send_command("auto_update_tick", serde_json::json!({})).await;
    }

    match cli.command {
        Commands::Backup { project } => cmd_backup(project, cli.json).await,
        Commands::Status => cmd_status(cli.json).await,
        Commands::List { project } => cmd_list(project, cli.json).await,
        Commands::Config(args) => cmd_config(args, cli.json).await,
        Commands::Pair => cmd_pair(),
        Commands::Snapshots { project } => cmd_snapshots(project, cli.json).await,
        Commands::Trust(args) => cmd_trust(args, cli.json).await,
        Commands::Update { check, from_daemon } => cmd_update(check, from_daemon, cli.json).await,
        Commands::Autoupdate(args) => cmd_autoupdate(args, cli.json).await,
    }
}

/// Runs the self-update worker on a blocking thread (it must keep working while
/// the daemon is stopped, so it cannot share the async runtime).
async fn cmd_update(check: bool, from_daemon: bool, json: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || update_worker::run_update(check, from_daemon, json))
        .await
        .map_err(|e| anyhow::anyhow!("update worker task failed: {e}"))?
}

/// Enables, disables, or shows automatic updates via the daemon settings.
async fn cmd_autoupdate(args: AutoupdateArgs, json: bool) -> Result<()> {
    match args.action {
        AutoupdateAction::On => set_autoupdate(true, json).await,
        AutoupdateAction::Off => set_autoupdate(false, json).await,
        AutoupdateAction::Status => {
            let v = client::send_command("get_update_settings", serde_json::json!({})).await?;
            if json {
                println!("{v}");
            } else {
                let on = v.get("auto_update").and_then(|b| b.as_bool()).unwrap_or(false);
                println!("auto-update: {}", if on { "on" } else { "off" });
            }
            Ok(())
        }
    }
}

/// Persists the auto-update preference through the daemon.
async fn set_autoupdate(on: bool, json: bool) -> Result<()> {
    let v = client::send_command(
        "set_update_settings",
        serde_json::json!({ "auto_update": on }),
    )
    .await?;
    if json {
        println!("{v}");
    } else {
        println!("auto-update {}", if on { "enabled" } else { "disabled" });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

/// Sends `run_backup` to the daemon and streams rsync progress to stdout.
///
/// # Parameters
///
/// - `project` — Optional project name; `None` backs up all projects.
/// - `json`    — When true, print the final JSON result instead of formatted output.
async fn cmd_backup(project: Option<String>, json: bool) -> Result<()> {
    let params = match project {
        Some(ref p) => serde_json::json!({ "project": p }),
        None => serde_json::json!({}),
    };
    let result = client::send_command_stream_progress("run_backup", params).await?;
    output::print_generic(&result, json);
    Ok(())
}

/// Calls `get_backup_status` and prints the result.
///
/// # Parameters
///
/// - `json` — When true, print raw JSON.
async fn cmd_status(json: bool) -> Result<()> {
    let result = client::send_command("get_backup_status", serde_json::json!({})).await?;
    output::print_status(&result, json);
    Ok(())
}

/// Lists projects or snapshots depending on whether a project name was provided.
///
/// - No project → calls `list_projects` and prints the project table.
/// - With project → calls `list_snapshots` and prints the snapshot list.
///
/// # Parameters
///
/// - `project` — Optional project name.
/// - `json`    — When true, print raw JSON.
async fn cmd_list(project: Option<String>, json: bool) -> Result<()> {
    match project {
        None => {
            // List all projects.
            let result = client::send_command(
                "list_projects",
                serde_json::json!({ "probe_remote": false }),
            )
            .await?;
            output::print_projects(&result, json);
        }
        Some(ref name) => {
            // List snapshots for the named project.
            let result = client::send_command(
                "list_snapshots",
                serde_json::json!({ "project": name }),
            )
            .await?;
            output::print_snapshots(&result, json);
        }
    }
    Ok(())
}

/// Handles the `config` subcommand group.
///
/// `config get <KEY>` retrieves the full config from the daemon and extracts a
/// dot-separated key path from the resulting JSON.
/// `config set <KEY> <VALUE>` is a stub that advises the user to use the GUI.
///
/// # Parameters
///
/// - `args` — Parsed `ConfigArgs` (wraps the `ConfigAction`).
/// - `json` — When true, print raw JSON for `get`.
async fn cmd_config(args: ConfigArgs, json: bool) -> Result<()> {
    match args.action {
        ConfigAction::Get { key } => {
            // Fetch the full config object from the daemon.
            let config = client::send_command("get_config", serde_json::json!({})).await?;
            if json {
                output::print_json_value(&config);
                return Ok(());
            }
            // Navigate the dot-separated key path into the config JSON.
            let value = resolve_json_path(&config, &key);
            match value {
                Some(v) => {
                    match v {
                        Value::String(s) => println!("{s}"),
                        Value::Null => println!("null"),
                        other => println!("{other}"),
                    }
                }
                None => bail!("key not found in config: {key}"),
            }
        }
        ConfigAction::Set { key, value: _ } => {
            eprintln!(
                "config set is not yet implemented in the CLI.\n\
                 To change '{key}', use the Backr GUI settings screen.\n\
                 (Patch-by-key-path without type reflection is deferred.)"
            );
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Navigates a dot-separated key path (e.g. `"remote.host"`) into a JSON value.
///
/// # Parameters
///
/// - `root` — The JSON object to navigate.
/// - `path` — Dot-separated field path string.
///
/// # Returns
///
/// `Some(&Value)` when every segment resolves; `None` when any segment is absent.
fn resolve_json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Prints a stub message for the `pair` subcommand.
///
/// Interactive terminal pairing (discover → confirm fingerprint → write config) is
/// deferred to the GUI wizard; the IPC calls exist but the TUI UX is not yet built.
///
/// # Returns
///
/// Always `Ok(())`.
fn cmd_pair() -> Result<()> {
    println!("Pairing wizard not yet interactive — use the GUI.");
    println!();
    println!("To pair from the command line you can manually call:");
    println!("  backrd IPC methods: discover_hosts, pair_with_host, confirm_pairing");
    Ok(())
}

/// Lists snapshots for the given project (or prompts if none given).
///
/// # Parameters
///
/// - `project` — Project name; required for `list_snapshots`.
/// - `json`    — When true, print raw JSON.
async fn cmd_snapshots(project: Option<String>, json: bool) -> Result<()> {
    let name = match project {
        Some(p) => p,
        None => bail!("a project name is required: backr snapshots <PROJECT>"),
    };
    let result = client::send_command(
        "list_snapshots",
        serde_json::json!({ "project": name }),
    )
    .await?;
    output::print_snapshots(&result, json);
    Ok(())
}

/// Handles the `trust` subcommand group (host-only operations).
///
/// Routes to `host_append_authorized_pubkey`, `host_list_authorized_pubkeys`,
/// or `host_remove_authorized_pubkey` depending on the sub-action.
///
/// # Parameters
///
/// - `args` — Parsed `TrustArgs`.
/// - `json` — When true, print raw JSON.
async fn cmd_trust(args: TrustArgs, json: bool) -> Result<()> {
    match args.action {
        TrustAction::Add { pubkey } => {
            let result = client::send_command(
                "host_append_authorized_pubkey",
                serde_json::json!({ "pubkey_line": pubkey }),
            )
            .await?;
            output::print_generic(&result, json);
        }
        TrustAction::List => {
            let result =
                client::send_command("host_list_authorized_pubkeys", serde_json::json!({}))
                    .await?;
            output::print_trust_list(&result, json);
        }
        TrustAction::Remove { raw_line } => {
            let result = client::send_command(
                "host_remove_authorized_pubkey",
                serde_json::json!({ "raw_line": raw_line }),
            )
            .await?;
            output::print_generic(&result, json);
        }
    }
    Ok(())
}
