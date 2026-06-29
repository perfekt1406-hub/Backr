/*
 * output.rs — Human-readable formatters for backrd IPC responses.
 *
 * Each public function receives a `&serde_json::Value` result payload (the `result`
 * field of a successful `IpcResponse`) and a `json` flag.  When `json` is `true`
 * the raw JSON is printed instead of the formatted table, which is useful for
 * scripting.
 *
 * All formatters degrade gracefully when expected fields are absent — they print
 * whatever is available rather than panicking.
 */

use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Prints `value` as pretty-printed JSON to stdout.
///
/// # Parameters
///
/// - `value` — Any JSON value to print.
fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

/// Returns a string from a JSON value's field, or a default if absent/not a string.
///
/// # Parameters
///
/// - `obj`     — JSON object to look up.
/// - `key`     — Field name.
/// - `default` — Fallback value.
///
/// # Returns
///
/// The string value or `default`.
fn str_field<'a>(obj: &'a Value, key: &str, default: &'a str) -> &'a str {
    obj.get(key).and_then(|v| v.as_str()).unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

/// Prints the backup status summary returned by `get_backup_status`.
///
/// Expected fields in `result`:
///   - `in_progress`   — bool
///   - `last_backup_at`  — RFC-3339 string or null
///   - `next_backup_at`  — RFC-3339 string or null
///   - `active_project`  — string or null
///
/// # Parameters
///
/// - `result` — `result` payload from the daemon response.
/// - `json`   — When true, print raw JSON instead of formatted text.
pub fn print_status(result: &Value, json: bool) {
    if json {
        print_json(result);
        return;
    }
    let in_progress = result
        .get("in_progress")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let last = str_field(result, "last_backup_at", "never");
    let next = str_field(result, "next_backup_at", "unknown");
    let active = result
        .get("active_project")
        .and_then(|v| v.as_str())
        .map(|s| format!(" (backing up: {s})"))
        .unwrap_or_default();

    println!("Backup status");
    println!("  Running:        {}{}", in_progress, active);
    println!("  Last backup:    {last}");
    println!("  Next scheduled: {next}");
}

/// Prints the list of projects returned by `list_projects`.
///
/// Each entry is expected to have:
///   - `name`           — string
///   - `last_backup_at` — RFC-3339 string or null
///   - `snapshot_count` — integer
///
/// # Parameters
///
/// - `result` — `result` payload (a JSON array).
/// - `json`   — When true, print raw JSON instead of formatted text.
pub fn print_projects(result: &Value, json: bool) {
    if json {
        print_json(result);
        return;
    }
    let projects = match result.as_array() {
        Some(arr) => arr,
        None => {
            eprintln!("Unexpected response format (expected an array).");
            return;
        }
    };
    if projects.is_empty() {
        println!("No projects found.");
        return;
    }
    println!("{:<30} {:>10}  {}", "Project", "Snapshots", "Last backup");
    println!("{}", "-".repeat(60));
    for p in projects {
        let name = str_field(p, "name", "<unknown>");
        let count = p
            .get("snapshot_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let last = str_field(p, "last_backup_at", "never");
        println!("{:<30} {:>10}  {}", name, count, last);
    }
}

/// Prints the snapshot list returned by `list_snapshots`.
///
/// Each entry is expected to have:
///   - `name` — a timestamp-encoded snapshot directory name (e.g. `2024-01-15_12-30-00`)
///
/// # Parameters
///
/// - `result` — `result` payload (a JSON array).
/// - `json`   — When true, print raw JSON instead of formatted text.
pub fn print_snapshots(result: &Value, json: bool) {
    if json {
        print_json(result);
        return;
    }
    let snapshots = match result.as_array() {
        Some(arr) => arr,
        None => {
            eprintln!("Unexpected response format (expected an array).");
            return;
        }
    };
    if snapshots.is_empty() {
        println!("No snapshots found.");
        return;
    }
    println!("Snapshots ({} total):", snapshots.len());
    for (i, s) in snapshots.iter().enumerate() {
        let name = str_field(s, "name", "<unknown>");
        // Pretty-print: replace underscores with spaces for readability.
        let display = name.replace('_', " ").replacen('-', "/", 2).replacen('-', "/", 1);
        println!("  {:3}. {}  ({})", i + 1, display, name);
    }
}

/// Prints the authorized-key list returned by `host_list_authorized_pubkeys`.
///
/// Each entry shape depends on `host_trust::host_list_authorized_pubkeys_impl`; at minimum
/// the raw `raw_line` field is expected.
///
/// # Parameters
///
/// - `result` — `result` payload (a JSON array).
/// - `json`   — When true, print raw JSON instead of formatted text.
pub fn print_trust_list(result: &Value, json: bool) {
    if json {
        print_json(result);
        return;
    }
    let entries = match result.as_array() {
        Some(arr) => arr,
        None => {
            eprintln!("Unexpected response format (expected an array).");
            return;
        }
    };
    if entries.is_empty() {
        println!("No trusted keys.");
        return;
    }
    println!("Trusted keys ({} total):", entries.len());
    for (i, e) in entries.iter().enumerate() {
        // The entry may carry `key_type`, `fingerprint`, `comment`, or just `raw_line`.
        let key_type = str_field(e, "key_type", "");
        let comment = str_field(e, "comment", "");
        let fp = str_field(e, "fingerprint", "");
        let raw = str_field(e, "raw_line", "");

        if !fp.is_empty() {
            println!("  {:3}. [{}] {} ({})", i + 1, key_type, comment, fp);
        } else if !raw.is_empty() {
            // Truncate very long raw lines for readability.
            let truncated = if raw.len() > 80 {
                format!("{}…", &raw[..77])
            } else {
                raw.to_string()
            };
            println!("  {:3}. {}", i + 1, truncated);
        } else {
            println!(
                "  {:3}. {}",
                i + 1,
                serde_json::to_string(e).unwrap_or_default()
            );
        }
    }
}

/// Prints `value` as pretty-printed JSON to stdout (public re-export for callers that
/// hold a raw value they want formatted as JSON unconditionally).
///
/// # Parameters
///
/// - `value` — Any JSON value to pretty-print.
pub fn print_json_value(value: &Value) {
    print_json(value);
}

/// Prints a generic result value when no specialised formatter exists.
///
/// Prints `null` results as "Done." and all other values as pretty JSON.
///
/// # Parameters
///
/// - `result` — `result` payload from the daemon response.
/// - `json`   — Ignored (always prints pretty JSON or "Done.").
pub fn print_generic(result: &Value, _json: bool) {
    match result {
        Value::Null => println!("Done."),
        other => print_json(other),
    }
}
