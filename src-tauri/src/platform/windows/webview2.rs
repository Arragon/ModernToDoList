//! WebView2 runtime detection with structured error reporting
//! (RD-M10-028~029 / INH-1119).
//!
//! The Tauri 2 Windows backend requires the Microsoft Edge WebView2 runtime.
//! This module detects it at startup and reports problems as a typed
//! [`WebView2Error`] enum — callers must never string-match on messages.
//!
//! Detection order:
//! 1. Fixed-version runtime bundled next to the app (`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`).
//! 2. Evergreen runtime registered per-machine or per-user in the registry
//!    (queried via `reg.exe` to avoid adding a winreg dependency).
//!
//! The offline fixed-version package policy — including user-facing download
//! instructions — is documented in `docs/release/WEBVIEW2_POLICY.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// Evergreen runtime client registry key (WOW6432Node view covers both
/// 32/64-bit registration for the Edge updater client).
pub const EVERGREEN_CLIENT_KEY_WOW: &str =
    r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
/// Evergreen runtime client registry key (native view).
pub const EVERGREEN_CLIENT_KEY_NATIVE: &str =
    r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
/// Environment variable that pins Tauri/WebView2 to a fixed-version runtime.
pub const FIXED_VERSION_ENV_VAR: &str = "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER";
/// Minimum Evergreen version supported by Tauri 2 / WebView2 SDK used at 2.0.0.
pub const MINIMUM_VERSION: &str = "1.0.1823.32";
/// Official offline installer landing page (documented fallback).
pub const OFFICIAL_DOWNLOAD_PAGE: &str =
    "https://developer.microsoft.com/en-us/microsoft-edge/webview2/";

#[derive(Debug, Error)]
pub enum WebView2Error {
    #[error("WebView2 runtime is not installed (checked registry and fixed-version folder)")]
    RuntimeNotInstalled,

    #[error("WebView2 runtime version {found} is older than the minimum supported {minimum}")]
    VersionTooOld { found: String, minimum: String },

    #[error("Fixed-version runtime folder is missing the WebView2 executable: {path}")]
    FixedVersionIncomplete { path: PathBuf },

    #[error("Registry query failed: {reason}")]
    RegistryQueryFailed { reason: String },

    #[error("Unparseable WebView2 version string: '{raw}'")]
    InvalidVersionString { raw: String },

    #[error("WebView2 detection is only supported on Windows")]
    UnsupportedPlatform,
}

pub type WebView2Result<T> = Result<T, WebView2Error>;

/// Which runtime channel was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebView2Channel {
    /// Auto-updating runtime shipped with Windows/Edge.
    Evergreen,
    /// Pinned offline runtime folder (portable/air-gapped policy).
    FixedVersion,
}

/// Structured information about a detected runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebView2RuntimeInfo {
    pub channel: WebView2Channel,
    /// Dotted version, e.g. `120.0.2210.91`. `None` for fixed-version folders
    /// whose manifest could not be read (presence is still authoritative).
    pub version: Option<String>,
    /// Install location when known.
    pub install_location: Option<PathBuf>,
}

/// Parse a dotted numeric version (`a.b.c.d`, 1–4 parts) into a comparable form.
pub fn parse_version(raw: &str) -> WebView2Result<[u32; 4]> {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return Err(WebView2Error::InvalidVersionString { raw: raw.to_string() });
    }
    let mut out = [0u32; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .parse::<u32>()
            .map_err(|_| WebView2Error::InvalidVersionString { raw: raw.to_string() })?;
    }
    Ok(out)
}

/// Compare two dotted version strings. Errors on malformed input.
pub fn compare_versions(a: &str, b: &str) -> WebView2Result<std::cmp::Ordering> {
    Ok(parse_version(a)?.cmp(&parse_version(b)?))
}

/// True when `version` satisfies the minimum supported runtime version.
pub fn version_is_supported(version: &str) -> WebView2Result<bool> {
    Ok(compare_versions(version, MINIMUM_VERSION)? != std::cmp::Ordering::Less)
}

/// Check a fixed-version runtime folder. The folder must contain
/// `msedgewebview2.exe`.
pub fn check_fixed_version_folder(dir: &Path) -> WebView2Result<WebView2RuntimeInfo> {
    let exe = dir.join("msedgewebview2.exe");
    if !exe.is_file() {
        return Err(WebView2Error::FixedVersionIncomplete { path: exe });
    }
    // Best-effort version read from the folder name convention
    // `Microsoft.WebView2.FixedVersionRuntime.<major>.<minor>.<build>.<rev>.x64`.
    let version = dir.file_name().and_then(|n| {
        let name = n.to_string_lossy().to_string();
        let parts: Vec<&str> = name.split('.').collect();
        // [Microsoft, WebView2, FixedVersionRuntime, a, b, c, d, arch]
        if parts.len() >= 7 && parts[..3] == ["Microsoft", "WebView2", "FixedVersionRuntime"] {
            let candidate = parts[3..7].join(".");
            if parse_version(&candidate).is_ok() {
                return Some(candidate);
            }
        }
        None
    });

    Ok(WebView2RuntimeInfo {
        channel: WebView2Channel::FixedVersion,
        version,
        install_location: Some(dir.to_path_buf()),
    })
}

/// Query the Evergreen runtime version from the registry using `reg.exe`.
/// Returns `Ok(None)` when the client key exists but reports version `0.0.0.0`
/// (the documented "not installed" sentinel).
#[cfg(target_os = "windows")]
pub fn query_evergreen_version() -> WebView2Result<Option<String>> {
    for hive in ["HKLM", "HKCU"] {
        for key in [EVERGREEN_CLIENT_KEY_WOW, EVERGREEN_CLIENT_KEY_NATIVE] {
            let output = Command::new("reg")
                .args(["query", &format!("{hive}\\{key}"), "/v", "pv"])
                .output()
                .map_err(|e| WebView2Error::RegistryQueryFailed {
                    reason: format!("cannot run reg.exe: {e}"),
                })?;
            if !output.status.success() {
                continue; // key not present in this hive/view — try the next
            }
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(version) = extract_pv_value(&text) {
                if version == "0.0.0.0" {
                    return Ok(None);
                }
                parse_version(&version)?; // validate shape
                return Ok(Some(version));
            }
        }
    }
    Ok(None)
}

/// Parse `reg query` stdout for the `pv` REG_SZ value.
fn extract_pv_value(reg_output: &str) -> Option<String> {
    for line in reg_output.lines() {
        let mut parts = line.split_whitespace();
        // Format: "    pv    REG_SZ    120.0.2210.91"
        if parts.next() == Some("pv") {
            let rest: Vec<&str> = parts.collect();
            if rest.len() >= 2 && rest[0] == "REG_SZ" {
                return Some(rest[1].to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn query_evergreen_version() -> WebView2Result<Option<String>> {
    Err(WebView2Error::UnsupportedPlatform)
}

/// Detect the WebView2 runtime available to this process.
///
/// Precedence: `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` (fixed version, per the
/// offline policy) → Evergreen registry lookup.
pub fn detect_runtime() -> WebView2Result<WebView2RuntimeInfo> {
    if let Ok(folder) = std::env::var(FIXED_VERSION_ENV_VAR) {
        if !folder.trim().is_empty() {
            // A pinned folder is authoritative: if it is broken, report that
            // instead of silently falling back to Evergreen.
            return check_fixed_version_folder(Path::new(&folder));
        }
    }

    match query_evergreen_version()? {
        Some(version) => {
            if !version_is_supported(&version)? {
                return Err(WebView2Error::VersionTooOld {
                    found: version,
                    minimum: MINIMUM_VERSION.to_string(),
                });
            }
            Ok(WebView2RuntimeInfo {
                channel: WebView2Channel::Evergreen,
                version: Some(version),
                install_location: None,
            })
        }
        None => Err(WebView2Error::RuntimeNotInstalled),
    }
}

/// User-facing remediation text for a given error — the download-instructions
/// fallback required by RD-M10-029. Shown in a dialog when detection fails.
pub fn user_instructions(err: &WebView2Error) -> String {
    match err {
        WebView2Error::RuntimeNotInstalled | WebView2Error::VersionTooOld { .. } => format!(
            "ModernToDoList requires the Microsoft Edge WebView2 Runtime.\n\n\
             How to install it:\n\
             1. On a computer with internet access, open:\n\
             \x20  {OFFICIAL_DOWNLOAD_PAGE}\n\
             2. Under 'Download the WebView2 Runtime', choose the\n\
             \x20  'Evergreen Standalone Installer' (x64).\n\
             3. Copy the installer to this computer and run it\n\
             \x20  (no administrator rights are required for the per-user installer).\n\
             4. Start ModernToDoList again.\n\n\
             Air-gapped machines: ask your IT administrator for the offline\n\
             fixed-version package described in docs/release/WEBVIEW2_POLICY.md."
        ),
        WebView2Error::FixedVersionIncomplete { path } => format!(
            "The pinned fixed-version WebView2 folder is incomplete:\n\
             {}\n\
             is missing msedgewebview2.exe.\n\n\
             Either restore the full fixed-version runtime folder, or unset\n\
             {FIXED_VERSION_ENV_VAR} to use the system Evergreen runtime.\n\
             Download fixed-version packages from:\n\
             \x20{OFFICIAL_DOWNLOAD_PAGE}",
            path.display()
        ),
        WebView2Error::RegistryQueryFailed { reason } => format!(
            "Could not query the Windows registry for the WebView2 runtime\n\
             ({reason}).\n\
             Please verify manually in 'Settings → Apps' that\n\
             'Microsoft Edge WebView2 Runtime' is installed, or reinstall it\n\
             from:\n  {OFFICIAL_DOWNLOAD_PAGE}"
        ),
        WebView2Error::InvalidVersionString { raw } => format!(
            "The detected WebView2 version string '{raw}' could not be parsed.\n\
             Please reinstall the runtime from:\n  {OFFICIAL_DOWNLOAD_PAGE}"
        ),
        WebView2Error::UnsupportedPlatform => {
            "WebView2 detection is only available on Windows.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_accepts_valid_forms() {
        assert_eq!(parse_version("120.0.2210.91").unwrap(), [120, 0, 2210, 91]);
        assert_eq!(parse_version("1.0").unwrap(), [1, 0, 0, 0]);
    }

    #[test]
    fn parse_version_rejects_garbage() {
        for bad in ["", "abc", "1.2.3.4.5", "12.0.0-beta"] {
            assert!(
                matches!(
                    parse_version(bad),
                    Err(WebView2Error::InvalidVersionString { .. })
                ),
                "'{bad}' should be rejected"
            );
        }
    }

    #[test]
    fn compare_versions_orders_correctly() {
        use std::cmp::Ordering::*;
        assert_eq!(
            compare_versions("120.0.0.0", "119.9.9.9").unwrap(),
            Greater
        );
        assert_eq!(
            compare_versions("1.0.1823.32", "1.0.1823.32").unwrap(),
            Equal
        );
        assert_eq!(compare_versions("1.0.1823.31", MINIMUM_VERSION).unwrap(), Less);
    }

    #[test]
    fn minimum_version_policy() {
        assert!(version_is_supported(MINIMUM_VERSION).unwrap());
        assert!(version_is_supported("126.0.2592.68").unwrap());
        assert!(!version_is_supported("1.0.1020.30").unwrap());
    }

    #[test]
    fn reg_output_parsing() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\...\r\n\r\n    pv    REG_SZ    120.0.2210.91\r\n\r\n";
        assert_eq!(
            extract_pv_value(out).as_deref(),
            Some("120.0.2210.91")
        );
        assert_eq!(extract_pv_value("no match here"), None);
    }

    #[test]
    fn fixed_version_folder_requires_executable() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_fixed_version_folder(tmp.path()).unwrap_err();
        assert!(matches!(err, WebView2Error::FixedVersionIncomplete { .. }));

        std::fs::write(tmp.path().join("msedgewebview2.exe"), b"stub").unwrap();
        let info = check_fixed_version_folder(tmp.path()).unwrap();
        assert_eq!(info.channel, WebView2Channel::FixedVersion);
        assert_eq!(info.install_location.as_deref(), Some(tmp.path()));
    }

    #[test]
    fn fixed_version_extracts_version_from_folder_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp
            .path()
            .join("Microsoft.WebView2.FixedVersionRuntime.120.0.2210.91.x64");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("msedgewebview2.exe"), b"stub").unwrap();
        let info = check_fixed_version_folder(&dir).unwrap();
        assert_eq!(info.version.as_deref(), Some("120.0.2210.91"));
    }

    #[test]
    fn detect_runtime_returns_structured_result() {
        // On a dev Windows machine this is usually Ok(Evergreen); the contract
        // is that failures are typed, not string-matched.
        match detect_runtime() {
            Ok(info) => {
                assert!(info.channel == WebView2Channel::Evergreen
                    || info.channel == WebView2Channel::FixedVersion);
            }
            Err(e) => {
                let text = user_instructions(&e);
                assert!(text.contains(OFFICIAL_DOWNLOAD_PAGE) || matches!(e, WebView2Error::UnsupportedPlatform));
            }
        }
    }

    #[test]
    fn user_instructions_cover_every_error_variant() {
        let cases = vec![
            WebView2Error::RuntimeNotInstalled,
            WebView2Error::VersionTooOld {
                found: "1.0.0.0".into(),
                minimum: MINIMUM_VERSION.into(),
            },
            WebView2Error::FixedVersionIncomplete {
                path: PathBuf::from("C:/wv2"),
            },
            WebView2Error::RegistryQueryFailed {
                reason: "reg.exe missing".into(),
            },
            WebView2Error::InvalidVersionString { raw: "x".into() },
            WebView2Error::UnsupportedPlatform,
        ];
        for e in cases {
            assert!(!user_instructions(&e).is_empty());
        }
    }
}
