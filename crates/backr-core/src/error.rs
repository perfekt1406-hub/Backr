/*
 * Error types for Backr backend operations.
 *
 * Two complementary types live here:
 *
 *  - `BackrError`        — internal structured error used by config, SSH, rsync, and filesystem
 *                          helpers.  NOT serialized; converted to `BackrCommandError` at the Tauri
 *                          command boundary.
 *
 *  - `BackrCommandError` — serializable error sent over the Tauri IPC channel as
 *                          `{ "kind": "<Kind>", "message": "<human text>" }`.  The frontend can
 *                          route on `kind` to show context-aware error UI.
 */

use serde::Serialize;
use thiserror::Error;

// ─── Internal helper error (unchanged public API) ────────────────────────────

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

// ─── Serializable IPC error ───────────────────────────────────────────────────

/// Discriminator tag for [`BackrCommandError`].
///
/// Each variant maps to the literal string emitted as `kind` in the IPC JSON payload so the
/// TypeScript frontend can switch on a stable set of identifiers without parsing message strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ErrorKind {
    /// No configuration has been saved yet; user must complete setup wizard.
    NotConfigured,
    /// An SSH command (key check, remote list, remote read, or connection probe) failed.
    SshFailed,
    /// An rsync process exited with a non-zero status or produced unexpected output.
    RsyncFailed,
    /// A local filesystem I/O operation failed.
    Io,
    /// A backup or restore job was rejected because another one is already running.
    BackupInProgress,
    /// A caller-supplied argument (snapshot name, file path, port, etc.) is invalid.
    InvalidInput,
    /// Reading or writing the configuration file (TOML parse, disk error, path expansion).
    Config,
    /// A pairing operation (mDNS, HTTP handshake, key exchange) failed.
    Pairing,
    /// A background task (`tokio::spawn_blocking`) could not be joined.
    TaskFailed,
}

/// Serializable error returned from every Tauri command.
///
/// Tauri serializes `Result<T, BackrCommandError>` as `{ "kind": "…", "message": "…" }` when the
/// result is `Err`, giving the TypeScript frontend a stable, routable error shape.
#[derive(Debug, Serialize)]
pub struct BackrCommandError {
    /// Stable discriminant the frontend routes on (see [`ErrorKind`]).
    pub kind: ErrorKind,
    /// Human-readable description for display or logging.
    pub message: String,
}

impl BackrCommandError {
    /// Constructs a `NotConfigured` error with a standard message.
    ///
    /// # Returns
    ///
    /// `BackrCommandError { kind: NotConfigured, message: "…" }`
    pub fn not_configured() -> Self {
        Self {
            kind: ErrorKind::NotConfigured,
            message: "application is not configured — complete the setup wizard first".into(),
        }
    }

    /// Constructs an `SshFailed` error wrapping a description string.
    ///
    /// # Inputs
    ///
    /// * `msg` — description of the SSH failure (command, host, or stderr excerpt).
    pub fn ssh_failed(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::SshFailed,
            message: msg.into(),
        }
    }

    /// Constructs an `RsyncFailed` error wrapping a description string.
    ///
    /// # Inputs
    ///
    /// * `msg` — rsync exit status or stderr excerpt.
    pub fn rsync_failed(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::RsyncFailed,
            message: msg.into(),
        }
    }

    /// Constructs an `Io` error from an I/O description string.
    ///
    /// # Inputs
    ///
    /// * `msg` — filesystem operation description and OS error text.
    pub fn io(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: msg.into(),
        }
    }

    /// Constructs a `BackupInProgress` error with a fixed message.
    pub fn backup_in_progress() -> Self {
        Self {
            kind: ErrorKind::BackupInProgress,
            message: "a backup is already in progress".into(),
        }
    }

    /// Constructs an `InvalidInput` error describing the rejected input.
    ///
    /// # Inputs
    ///
    /// * `msg` — description of what was invalid and why.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: msg.into(),
        }
    }

    /// Constructs a `Config` error for configuration read/write failures.
    ///
    /// # Inputs
    ///
    /// * `msg` — human description of the config error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Config,
            message: msg.into(),
        }
    }

    /// Constructs a `Pairing` error for mDNS/HTTP/key-exchange failures.
    ///
    /// # Inputs
    ///
    /// * `msg` — description of the pairing failure.
    pub fn pairing(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Pairing,
            message: msg.into(),
        }
    }

    /// Constructs a `TaskFailed` error for `tokio::spawn_blocking` join errors.
    ///
    /// # Inputs
    ///
    /// * `msg` — join error description.
    pub fn task_failed(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::TaskFailed,
            message: msg.into(),
        }
    }
}

// ─── Conversions into BackrCommandError ──────────────────────────────────────

impl From<String> for BackrCommandError {
    /// Promotes a bare `String` error into an `InvalidInput` command error.
    ///
    /// Used as the ergonomic fallback when the error origin is unknown or already formatted.
    fn from(s: String) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: s,
        }
    }
}

impl From<&str> for BackrCommandError {
    /// Promotes a `&str` error into an `InvalidInput` command error.
    fn from(s: &str) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: s.to_string(),
        }
    }
}

impl From<std::io::Error> for BackrCommandError {
    /// Converts a standard I/O error into an `Io` command error.
    fn from(e: std::io::Error) -> Self {
        Self::io(e.to_string())
    }
}

impl From<BackrError> for BackrCommandError {
    /// Converts an internal `BackrError` into the appropriate `BackrCommandError` kind.
    ///
    /// The mapping preserves semantic intent so the frontend receives the right discriminant:
    ///
    /// | `BackrError`         | `BackrCommandError::kind` |
    /// |----------------------|---------------------------|
    /// | `Config`             | `Config`                  |
    /// | `Io`                 | `Io`                      |
    /// | `Utf8`               | `Io`                      |
    /// | `TomlSer / TomlDe`   | `Config`                  |
    /// | `Remote`             | `SshFailed`               |
    /// | `Regex`              | `Config`                  |
    /// | `BackupInProgress`   | `BackupInProgress`        |
    /// | `Msg`                | `InvalidInput`            |
    fn from(e: BackrError) -> Self {
        match e {
            BackrError::Config(m) => Self::config(m),
            BackrError::Io(e) => Self::io(e.to_string()),
            BackrError::Utf8(e) => Self::io(e.to_string()),
            BackrError::TomlSer(e) => Self::config(e.to_string()),
            BackrError::TomlDe(e) => Self::config(e.to_string()),
            BackrError::Remote(m) => Self::ssh_failed(m),
            BackrError::Regex(e) => Self::config(e.to_string()),
            BackrError::BackupInProgress => Self::backup_in_progress(),
            BackrError::Msg(m) => Self::invalid_input(m),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `kind` field serializes to the stable string expected by the TypeScript frontend.
    #[test]
    fn kind_serializes_to_stable_strings() {
        assert_eq!(
            serde_json::to_string(&ErrorKind::NotConfigured).unwrap(),
            r#""NotConfigured""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::SshFailed).unwrap(),
            r#""SshFailed""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::RsyncFailed).unwrap(),
            r#""RsyncFailed""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::Io).unwrap(),
            r#""Io""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::BackupInProgress).unwrap(),
            r#""BackupInProgress""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::InvalidInput).unwrap(),
            r#""InvalidInput""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::Config).unwrap(),
            r#""Config""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::Pairing).unwrap(),
            r#""Pairing""#
        );
        assert_eq!(
            serde_json::to_string(&ErrorKind::TaskFailed).unwrap(),
            r#""TaskFailed""#
        );
    }

    /// `BackrCommandError::not_configured()` produces the `NotConfigured` kind.
    #[test]
    fn not_configured_has_correct_kind() {
        let err = BackrCommandError::not_configured();
        assert_eq!(err.kind, ErrorKind::NotConfigured);
        assert!(!err.message.is_empty());
    }

    /// `BackrCommandError` serializes to a `{ kind, message }` JSON object.
    #[test]
    fn command_error_serializes_to_kind_message_shape() {
        let err = BackrCommandError::not_configured();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "NotConfigured");
        assert!(json["message"].as_str().is_some());
    }

    /// `BackrError::Remote` converts to `SshFailed`.
    #[test]
    fn backr_error_remote_converts_to_ssh_failed() {
        let cmd_err = BackrCommandError::from(BackrError::Remote("connection refused".into()));
        assert_eq!(cmd_err.kind, ErrorKind::SshFailed);
    }

    /// `BackrError::BackupInProgress` converts to `BackupInProgress`.
    #[test]
    fn backr_error_backup_in_progress_converts_correctly() {
        let cmd_err = BackrCommandError::from(BackrError::BackupInProgress);
        assert_eq!(cmd_err.kind, ErrorKind::BackupInProgress);
    }

    /// `From<std::io::Error>` produces `Io` kind.
    #[test]
    fn from_io_error_produces_io_kind() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let cmd_err = BackrCommandError::from(io_err);
        assert_eq!(cmd_err.kind, ErrorKind::Io);
    }

    /// `From<String>` produces `InvalidInput` kind.
    #[test]
    fn from_string_produces_invalid_input_kind() {
        let cmd_err = BackrCommandError::from("something went wrong".to_string());
        assert_eq!(cmd_err.kind, ErrorKind::InvalidInput);
    }
}
