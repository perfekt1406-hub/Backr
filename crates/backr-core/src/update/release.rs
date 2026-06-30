/*
 * GitHub Releases client for the self-update flow.
 *
 * Resolves the latest release for the configured repo, compares its tag against
 * the running version (KTD2, semver), selects the Linux binaries that match the
 * host architecture, downloads them, and verifies SHA-256 checksums BEFORE they
 * are handed to the swap step (KTD5). Network (ureq + rustls) and hashing (sha2)
 * are in-process on purpose: verify-before-swap is the security boundary for
 * replacing the running daemon, so it must be typed, testable code rather than a
 * parsed `curl`/`sha256sum` subprocess.
 *
 * Blocking I/O: callers on an async runtime must run these off the runtime
 * (e.g. tokio::task::spawn_blocking).
 */

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::BackrError;

/// Default repo the client checks for releases; overridable via `BACKR_REPO_SLUG`.
pub const DEFAULT_REPO_SLUG: &str = "perfekt1406-hub/Backr";

/// The three binaries a client update replaces (swapped in lockstep — KTD: U5).
pub const RELEASE_BINARIES: [&str; 3] = ["backrd", "backr-app", "backr"];

/// Name of the checksum manifest asset published alongside the binaries (U11).
pub const CHECKSUM_ASSET: &str = "SHA256SUMS";

/// User-Agent the GitHub API requires; carries the running version for debugging.
const USER_AGENT: &str = concat!("backr-updater/", env!("CARGO_PKG_VERSION"));

/// A downloadable asset attached to a GitHub release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

/// Parsed view of a GitHub release, limited to what the updater needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub assets: Vec<ReleaseAsset>,
}

impl ReleaseInfo {
    /// Finds an asset by exact name.
    pub fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// Finds the binary asset matching `bin` for the host architecture.
    pub fn binary_asset(&self, bin: &str) -> Option<&ReleaseAsset> {
        self.asset(&asset_name(bin, target_arch()))
    }

    /// Finds the checksum manifest asset, if present.
    pub fn checksum_asset(&self) -> Option<&ReleaseAsset> {
        self.asset(CHECKSUM_ASSET)
    }
}

/// The repo slug the client checks for releases (`BACKR_REPO_SLUG` or the default).
pub fn repo_slug() -> String {
    std::env::var("BACKR_REPO_SLUG").unwrap_or_else(|_| DEFAULT_REPO_SLUG.to_string())
}

/// Optional GitHub token (`BACKR_GITHUB_TOKEN`) used to raise the API rate limit.
pub fn github_token() -> Option<String> {
    std::env::var("BACKR_GITHUB_TOKEN").ok().filter(|s| !s.is_empty())
}

/// Host architecture token used in asset names (e.g. `x86_64`, `aarch64`).
pub fn target_arch() -> &'static str {
    std::env::consts::ARCH
}

/// Builds the conventional release asset name: `<bin>-linux-<arch>` (Linux-only for now).
pub fn asset_name(bin: &str, arch: &str) -> String {
    format!("{bin}-linux-{arch}")
}

/// Parses a release tag or version string into a comparable `(major, minor, patch)`.
///
/// Tolerates an optional leading `v` and ignores any pre-release/build suffix
/// (`1.2.3-rc1`, `1.2.3+meta`). Returns `None` when the core is not `X.Y.Z`.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let t = s.trim();
    let t = t.strip_prefix('v').unwrap_or(t);
    let core = t.split(|c| c == '-' || c == '+').next().unwrap_or(t);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true when `latest` is a strictly newer semver than `current`.
///
/// Unparseable inputs are treated as "not newer" so a malformed tag never
/// triggers an update.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Parses a GitHub `releases/latest` JSON body into a [`ReleaseInfo`].
///
/// Split from the network call so JSON handling is unit-testable with fixtures.
pub fn parse_release_json(json: &str) -> Result<ReleaseInfo, BackrError> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| BackrError::Update(format!("could not parse release JSON: {e}")))?;
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| BackrError::Update("release JSON missing tag_name".into()))?
        .to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let name = a.get("name")?.as_str()?.to_string();
                    let url = a.get("browser_download_url")?.as_str()?.to_string();
                    Some(ReleaseAsset { name, url })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ReleaseInfo { tag, assets })
}

/// Parses `sha256sum`-style manifest text ("<hex>  <name>") into name → lowercase hex.
pub fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((hex, rest)) = line.split_once(char::is_whitespace) {
            // The name field may be prefixed by a space or `*` (binary mode marker).
            let name = rest.trim_start_matches([' ', '*']).trim();
            if hex.len() == 64 && !name.is_empty() {
                map.insert(name.to_string(), hex.to_ascii_lowercase());
            }
        }
    }
    map
}

/// Lowercase hex SHA-256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Verifies that the file at `path` hashes to `expected_hex` (case-insensitive).
///
/// Returns an `Update` error on mismatch so the caller refuses the swap (KTD5).
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<(), BackrError> {
    let bytes = std::fs::read(path).map_err(BackrError::Io)?;
    let actual = sha256_hex(&bytes);
    if actual.eq_ignore_ascii_case(expected_hex.trim()) {
        Ok(())
    } else {
        Err(BackrError::Update(format!(
            "checksum mismatch for {}: expected {}, got {}",
            path.display(),
            expected_hex.trim(),
            actual
        )))
    }
}

/// Builds an HTTP agent with end-to-end timeouts (rustls TLS by default).
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Fetches the latest release for `slug` from the GitHub API.
///
/// # Inputs
/// * `slug` — `owner/repo`.
/// * `token` — optional GitHub token for a higher rate limit.
pub fn fetch_latest_release(slug: &str, token: Option<&str>) -> Result<ReleaseInfo, BackrError> {
    let url = format!("https://api.github.com/repos/{slug}/releases/latest");
    let agent = http_agent();
    let mut req = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json");
    if let Some(tok) = token {
        req = req.header("Authorization", &format!("Bearer {tok}"));
    }
    let mut resp = req
        .call()
        .map_err(|e| BackrError::Update(format!("release lookup failed for {slug}: {e}")))?;
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| BackrError::Update(format!("reading release response failed: {e}")))?;
    parse_release_json(&body)
}

/// Downloads `url` to `dest`, capped to guard against an oversized/malicious response.
pub fn download_to_file(url: &str, token: Option<&str>, dest: &Path) -> Result<(), BackrError> {
    let agent = http_agent();
    let mut req = agent.get(url).header("User-Agent", USER_AGENT);
    if let Some(tok) = token {
        req = req.header("Authorization", &format!("Bearer {tok}"));
    }
    let mut resp = req
        .call()
        .map_err(|e| BackrError::Update(format!("download failed for {url}: {e}")))?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| BackrError::Update(format!("reading download failed for {url}: {e}")))?;
    std::fs::write(dest, &bytes).map_err(BackrError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_tolerates_v_prefix_and_suffix() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version(" v0.4.10 "), Some((0, 4, 10)));
        assert_eq!(parse_version("v1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3+build7"), Some((1, 2, 3)));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version("1.2"), None);
    }

    #[test]
    fn is_newer_compares_semver_and_is_safe_on_garbage() {
        assert!(is_newer("v1.2.3", "v1.2.2"));
        assert!(is_newer("v1.3.0", "v1.2.9"));
        assert!(is_newer("v2.0.0", "v1.9.9"));
        assert!(!is_newer("v1.2.3", "v1.2.3"));
        assert!(!is_newer("v1.2.2", "v1.2.3"));
        assert!(!is_newer("garbage", "v1.2.3"));
        assert!(!is_newer("v1.2.3", "garbage"));
    }

    #[test]
    fn binary_asset_selects_by_arch() {
        let arch = target_arch();
        let rel = ReleaseInfo {
            tag: "v1.0.0".into(),
            assets: vec![
                ReleaseAsset { name: asset_name("backrd", arch), url: "u1".into() },
                ReleaseAsset { name: asset_name("backr-app", arch), url: "u2".into() },
                ReleaseAsset { name: "backrd-linux-someotherarch".into(), url: "u3".into() },
                ReleaseAsset { name: CHECKSUM_ASSET.into(), url: "u4".into() },
            ],
        };
        assert_eq!(rel.binary_asset("backrd").unwrap().url, "u1");
        assert_eq!(rel.binary_asset("backr-app").unwrap().url, "u2");
        assert!(rel.binary_asset("does-not-exist").is_none());
        assert_eq!(rel.checksum_asset().unwrap().url, "u4");
    }

    #[test]
    fn parse_release_json_extracts_tag_and_assets() {
        let json = r#"{
            "tag_name": "v0.4.2",
            "assets": [
                {"name": "backrd-linux-x86_64", "browser_download_url": "https://x/backrd"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/sums"}
            ]
        }"#;
        let rel = parse_release_json(json).expect("parse");
        assert_eq!(rel.tag, "v0.4.2");
        assert_eq!(rel.assets.len(), 2);
        assert_eq!(rel.asset("backrd-linux-x86_64").unwrap().url, "https://x/backrd");
    }

    #[test]
    fn parse_release_json_rejects_missing_tag() {
        assert!(parse_release_json(r#"{"assets": []}"#).is_err());
    }

    #[test]
    fn parse_sha256sums_reads_hex_and_names() {
        let manifest = "\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  backrd-linux-x86_64
ABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCD *backr-linux-x86_64
";
        let map = parse_sha256sums(manifest);
        assert_eq!(
            map.get("backrd-linux-x86_64").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // `*` binary-mode marker is stripped and hex is lowercased.
        assert_eq!(
            map.get("backr-linux-x86_64").unwrap(),
            "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"
        );
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        // SHA-256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_sha256_accepts_match_and_rejects_mismatch() {
        let dir = std::env::temp_dir().join(format!("backr-verify-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob");
        std::fs::write(&path, b"abc").unwrap();

        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_sha256(&path, good).is_ok());
        assert!(verify_sha256(&path, &good.to_ascii_uppercase()).is_ok());
        assert!(verify_sha256(&path, "deadbeef").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
