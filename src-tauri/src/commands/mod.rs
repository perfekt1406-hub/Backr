/*
 * Tauri command handlers and shared DTOs for configuration, projects, backups, and snapshots.
 * Each submodule maps closely to the user-facing operations described in the product plan.
 */

pub mod activity_cmd;
pub mod backup_cmd;
pub mod config_cmd;
pub mod host_cmd;
pub mod project_cmd;
pub mod snapshot_cmd;
pub mod system_cmd;

pub use activity_cmd::get_activity_series;
pub use backup_cmd::{execute_backup_cycle_with_sink, spawn_backup_job};
pub use config_cmd::{get_config, save_config, test_connection};
pub use host_cmd::{host_list_snapshot_projects, host_volume_summary, resolve_shell_bootstrap};
pub use project_cmd::{get_backup_status, list_projects};
pub use snapshot_cmd::{
    list_files, list_snapshots, read_snapshot_file, restore_all_projects, restore_all_snapshots,
    restore_snapshot,
};
pub use system_cmd::get_system_info;

pub use backup_cmd::run_backup;
