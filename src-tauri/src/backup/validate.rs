/*
 * Centralized path-traversal validation for user-supplied identifiers.
 *
 * All identifiers that flow into SSH commands (project names, snapshot names,
 * file paths) must be validated at the command boundary before any SSH or
 * filesystem operation is attempted.  Shell-escaping and rsync `--protect-args`
 * are the first defence layer; these functions are the semantic second layer
 * that rejects traversal attempts (`../etc`) which would still name a valid
 * directory after escaping.
 *
 * Two validators are exported:
 *   - `validate_remote_component` — for single path segments (no `/`).
 *   - `validate_relative_path`    — for relative multi-segment paths inside
 *                                    a snapshot (`src/main.rs`).
 */

/// Validates a single path component (project name, snapshot name, etc.) that
/// must not traverse the filesystem.
///
/// Rejects:
///   - Empty strings
///   - `"."` (current directory alias)
///   - `".."` (parent directory traversal)
///   - Anything containing `/` (path separator — components must be flat)
///   - Anything containing `\` (Windows path separator)
///   - Anything containing NUL (`\0`) — terminates C strings / SSH args
///   - Anything containing newline (`\n`) — truncates remote shell argv tokens
///
/// Unicode codepoints that are not in the above list are accepted (e.g. `"Répertoire"`).
///
/// # Inputs
///
/// * `name` — the candidate path component string.
///
/// # Returns
///
/// `Ok(())` if the component is safe to use; `Err(String)` describing why it
/// was rejected.
pub fn validate_remote_component(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("path component must not be empty".into());
    }
    if name == "." {
        return Err("path component must not be \".\" (current directory)".into());
    }
    if name == ".." {
        return Err("path component must not be \"..\" (parent directory traversal)".into());
    }
    // Disallow path separators — this must be a single flat component, not a multi-segment path.
    if name.contains('/') {
        return Err("path component must not contain '/' (use validate_relative_path for multi-segment paths)".into());
    }
    if name.contains('\\') {
        return Err("path component must not contain '\\'".into());
    }
    // NUL terminates C strings and is rejected by OpenSSH argument handling.
    if name.contains('\0') {
        return Err("path component must not contain NUL bytes".into());
    }
    // Newlines truncate SSH remote argv tokens because the login shell splits on them.
    if name.contains('\n') {
        return Err("path component must not contain newline characters".into());
    }
    Ok(())
}

/// Validates a relative file path used inside a snapshot (e.g. `"src/main.rs"`).
///
/// Rules:
///   - Must not be empty.
///   - Must not be absolute (no leading `/`).
///   - Must not contain a `..` path segment (at any position).
///   - Must not contain NUL (`\0`) or newline (`\n`) characters.
///
/// Each `/`-separated segment is checked individually so that `a/../../b` is
/// rejected even though neither `a` nor `b` alone is `..`.
///
/// # Inputs
///
/// * `path` — the candidate relative path string.
///
/// # Returns
///
/// `Ok(())` if the path is safe; `Err(String)` describing why it was rejected.
pub fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("relative path must not be empty".into());
    }
    // Absolute paths are not relative — reject any leading slash.
    if path.starts_with('/') {
        return Err("relative path must not be absolute (no leading '/')".into());
    }
    // NUL terminates C strings and is rejected by OpenSSH argument handling.
    if path.contains('\0') {
        return Err("relative path must not contain NUL bytes".into());
    }
    // Newlines truncate SSH remote argv tokens because the login shell splits on them.
    if path.contains('\n') {
        return Err("relative path must not contain newline characters".into());
    }
    // Check every segment — `a/../../b` must be rejected even though the full
    // string does not start with `..`.
    for segment in path.split('/') {
        if segment == ".." {
            return Err(
                "relative path must not contain \"..\" segments (path traversal)".into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // validate_remote_component — happy paths
    // -------------------------------------------------------------------------

    /// Typical project and snapshot names must be accepted.
    #[test]
    fn component_happy_paths() {
        assert!(validate_remote_component("my-app").is_ok());
        assert!(validate_remote_component("Submissions (Copy)").is_ok());
        assert!(validate_remote_component("hello world").is_ok());
        assert!(validate_remote_component("project_name").is_ok());
        assert!(validate_remote_component("123").is_ok());
    }

    /// Unicode names that contain no forbidden characters must be accepted.
    #[test]
    fn component_unicode_accepted() {
        assert!(validate_remote_component("Répertoire").is_ok());
        assert!(validate_remote_component("项目").is_ok());
    }

    // -------------------------------------------------------------------------
    // validate_remote_component — edge cases and rejections
    // -------------------------------------------------------------------------

    /// Empty string must be rejected.
    #[test]
    fn component_rejects_empty() {
        assert!(validate_remote_component("").is_err());
    }

    /// Single dot must be rejected (current-directory alias).
    #[test]
    fn component_rejects_single_dot() {
        assert!(validate_remote_component(".").is_err());
    }

    /// Double dot must be rejected (parent-directory traversal).
    #[test]
    fn component_rejects_double_dot() {
        assert!(validate_remote_component("..").is_err());
    }

    /// A traversal string that contains slashes must be rejected.
    #[test]
    fn component_rejects_traversal_with_slash() {
        assert!(validate_remote_component("../etc").is_err());
    }

    /// Any component containing `/` must be rejected — use `validate_relative_path` instead.
    #[test]
    fn component_rejects_slash() {
        assert!(validate_remote_component("a/b").is_err());
        // A trailing slash is also forbidden (contains '/').
        assert!(validate_remote_component("project/").is_err());
    }

    /// Components containing NUL bytes must be rejected.
    #[test]
    fn component_rejects_nul() {
        assert!(validate_remote_component("bad\0name").is_err());
    }

    /// Components containing newline characters must be rejected.
    #[test]
    fn component_rejects_newline() {
        assert!(validate_remote_component("bad\nname").is_err());
    }

    // -------------------------------------------------------------------------
    // validate_relative_path — happy paths
    // -------------------------------------------------------------------------

    /// Typical file paths inside a snapshot must be accepted.
    #[test]
    fn relative_path_happy_paths() {
        assert!(validate_relative_path("src/main.rs").is_ok());
        assert!(validate_relative_path("dir/subdir/file.txt").is_ok());
        assert!(validate_relative_path("path with spaces/file").is_ok());
    }

    // -------------------------------------------------------------------------
    // validate_relative_path — edge cases and rejections
    // -------------------------------------------------------------------------

    /// Empty path must be rejected.
    #[test]
    fn relative_path_rejects_empty() {
        assert!(validate_relative_path("").is_err());
    }

    /// The lone `".."` string is a traversal segment and must be rejected.
    #[test]
    fn relative_path_rejects_double_dot_alone() {
        assert!(validate_relative_path("..").is_err());
    }

    /// Absolute paths (starting with `/`) must be rejected.
    #[test]
    fn relative_path_rejects_absolute() {
        assert!(validate_relative_path("/etc/passwd").is_err());
    }

    /// Mid-path `..` segments must be rejected even when surrounded by normal components.
    #[test]
    fn relative_path_rejects_mid_traversal() {
        assert!(validate_relative_path("a/../../b").is_err());
    }

    /// Relative path with a trailing `..` segment must be rejected.
    #[test]
    fn relative_path_rejects_trailing_traversal() {
        assert!(validate_relative_path("a/b/..").is_err());
    }

    /// NUL bytes in a relative path must be rejected.
    #[test]
    fn relative_path_rejects_nul() {
        assert!(validate_relative_path("src/\0bad").is_err());
    }

    /// Newline characters in a relative path must be rejected.
    #[test]
    fn relative_path_rejects_newline() {
        assert!(validate_relative_path("src/\nbad").is_err());
    }

    // -------------------------------------------------------------------------
    // Integration: command-level traversal rejections
    // -------------------------------------------------------------------------

    /// `list_files` and `read_snapshot_file` both reject `"../etc"` as the project
    /// argument — closes the traversal inconsistency described in U1.
    ///
    /// This test mirrors the validation that every command handler must perform at
    /// the top of its body before touching config or SSH.
    #[test]
    fn traversal_rejected_as_project_component() {
        // "../etc" contains '/' so it is not a valid single component.
        assert!(validate_remote_component("../etc").is_err());
    }

    /// A valid snapshot name passes component validation (independent of the
    /// stricter timestamp-format check in `ssh::is_valid_snapshot_name`).
    #[test]
    fn valid_snapshot_name_passes_component_check() {
        assert!(validate_remote_component("2026-05-11_09-30-45").is_ok());
    }
}
