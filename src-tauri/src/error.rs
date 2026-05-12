/*
 * Central error type for Backr backend operations.
 * Maps internal failures into user-facing strings for Tauri command surfaces.
 */

use thiserror::Error;

/// Structured errors produced by config, SSH, rsync, and filesystem helpers.
#[derive(Debug, Error)]
pub enum BackrError {
    /// Configuration file or field is missing or invalid for the requested operation.
    #[error("configuration error: {0}")]
    Config(String),

    /// I/O failures reading or writing local paths (config, projects directory, restore targets).
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// UTF-8 decode failures from process output.
    #[error("invalid utf-8 in command output: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// TOML serialization or deserialization failures.
    #[error("config parse/save failed: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// TOML deserialization failures.
    #[error("config parse failed: {0}")]
    TomlDe(#[from] toml::de::Error),

    /// Remote command or rsync exited with a non-zero status.
    #[error("remote command failed: {0}")]
    Remote(String),

    /// Regex compilation (snapshot name filter) failures.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Backup is already running (concurrency guard).
    #[error("a backup is already in progress")]
    BackupInProgress,

    /// Generic message bucket for rare paths that are still surfaced to users.
    #[error("{0}")]
    Msg(String),
}

impl BackrError {
    /// Converts this error into a single string suitable for `Result<T, String>` command returns.
    ///
    /// # Returns
    ///
    /// Human-readable description; same as `to_string()` but keeps the API explicit for callers.
    pub fn to_user_string(&self) -> String {
        self.to_string()
    }
}

impl From<BackrError> for String {
    /// Allows `?` in functions that return `Result<_, String>` when the error type is `BackrError`.
    fn from(value: BackrError) -> Self {
        value.to_string()
    }
}
