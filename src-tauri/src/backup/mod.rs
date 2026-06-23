/*
 * Backup subsystems: SSH inspection helpers and local rsync orchestration.
 * Re-exports symbols used by Tauri command handlers and the periodic scheduler.
 */

pub mod excludes;
pub mod rsync;
pub mod ssh;
