//! EXE version metadata (RD-M10-038 / INH-1123).
//!
//! Windows embeds version information into the PE resource section of the
//! executable (the "Details" tab in Explorer: File version, Product version,
//! Company, Description, Copyright). For Tauri 2 this metadata is produced
//! from `src-tauri/tauri.conf.json` + `src-tauri/Cargo.toml`:
//!
//! - `productName` / `version` in `tauri.conf.json` drive the Win32
//!   `VERSIONINFO` resource (File version & Product version).
//! - `identifier` becomes the `OriginalFilename`/company linkage.
//! - `bundle.windows` options control publisher display name and signing.
//!
//! This module is the single source of truth for those values. The release
//! generator writes `version-metadata.json`, and the orchestrator applies it
//! to `tauri.conf.json` (this module deliberately does NOT edit the config):
//!
//! ```json
//! {
//!   "productName": "ModernToDoList",
//!   "version": "2.0.0",
//!   "identifier": "com.moderntodolist.app",
//!   "bundle": {
//!     "windows": {
//!       "publisher": "ModernToDoList Team",
//!       "fileVersion": "2.0.0.0"
//!     }
//!   }
//! }
//! ```
//!
//! See [`VersionMetadata::render_tauri_conf_snippet`].

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Invalid version string '{raw}': expected MAJOR.MINOR.PATCH[-PRERELEASE]")]
    InvalidSemver { raw: String },

    #[error("Field '{field}' must not be empty")]
    EmptyField { field: &'static str },
}

pub type VersionResult<T> = Result<T, VersionError>;

/// Version metadata embedded into the released EXE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMetadata {
    /// Four-part Win32 file version, e.g. `2.0.0.0`.
    pub file_version: String,
    /// Product version (semver), e.g. `2.0.0`.
    pub product_version: String,
    /// Company / publisher shown in Explorer details and UAC prompts.
    pub company: String,
    /// Product name shown in Explorer details.
    pub product_name: String,
    /// One-line description.
    pub description: String,
    /// Copyright line.
    pub copyright: String,
    /// EXE base file name without directory, e.g. `ModernToDoList.exe`.
    pub original_filename: String,
}

impl VersionMetadata {
    /// The canonical metadata for the ModernToDoList 2.0.0 GA release.
    pub fn v2_0_0() -> Self {
        Self {
            file_version: "2.0.0.0".to_string(),
            product_version: "2.0.0".to_string(),
            company: "ModernToDoList Team".to_string(),
            product_name: "ModernToDoList".to_string(),
            description: "ModernToDoList - Portable Windows Desktop Task Manager".to_string(),
            copyright: "Copyright (C) 2026 ModernToDoList Team".to_string(),
            original_filename: "ModernToDoList.exe".to_string(),
        }
    }

    /// Build from a semver product version, deriving the four-part file version.
    pub fn from_semver(semver: &str, company: &str) -> VersionResult<Self> {
        let (major, minor, patch) = parse_semver(semver)?;
        Ok(Self {
            file_version: format!("{major}.{minor}.{patch}.0"),
            product_version: format!("{major}.{minor}.{patch}"),
            company: company.to_string(),
            product_name: "ModernToDoList".to_string(),
            description: "ModernToDoList - Portable Windows Desktop Task Manager".to_string(),
            copyright: format!("Copyright (C) 2026 {company}"),
            original_filename: "ModernToDoList.exe".to_string(),
        })
    }

    /// Validate all fields for use in a PE VERSIONINFO resource.
    pub fn validate(&self) -> VersionResult<()> {
        for (field, value) in [
            ("company", &self.company),
            ("productName", &self.product_name),
            ("description", &self.description),
            ("copyright", &self.copyright),
            ("originalFilename", &self.original_filename),
        ] {
            if value.trim().is_empty() {
                return Err(VersionError::EmptyField { field });
            }
        }
        let parts = self.four_part_file_version()?;
        if parts.iter().any(|p| *p > u16::MAX as u32) {
            return Err(VersionError::InvalidSemver {
                raw: self.file_version.clone(),
            });
        }
        parse_semver(&self.product_version)?;
        Ok(())
    }

    /// Parse `file_version` into its four numeric components.
    pub fn four_part_file_version(&self) -> VersionResult<[u32; 4]> {
        let mut parts = [0u32; 4];
        let split: Vec<&str> = self.file_version.split('.').collect();
        if split.len() != 4 {
            return Err(VersionError::InvalidSemver {
                raw: self.file_version.clone(),
            });
        }
        for (i, s) in split.iter().enumerate() {
            parts[i] = s.parse::<u32>().map_err(|_| VersionError::InvalidSemver {
                raw: self.file_version.clone(),
            })?;
        }
        Ok(parts)
    }

    /// Serialize as the release `version-metadata.json` document.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("VersionMetadata serializes")
    }

    /// Render the JSON fragment that the orchestrator merges into
    /// `src-tauri/tauri.conf.json` so the built EXE carries this metadata.
    pub fn render_tauri_conf_snippet(&self) -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "productName": self.product_name,
            "version": self.product_version,
            "identifier": "com.moderntodolist.app",
            "bundle": {
                "windows": {
                    "publisher": self.company,
                    "fileVersion": self.four_part_file_version()
                        .map(|p| format!("{}.{}.{}.{}", p[0], p[1], p[2], p[3]))
                        .unwrap_or_else(|_| self.file_version.clone()),
                },
                "shortDescription": self.description,
                "longDescription": self.description,
            },
        }))
        .expect("snippet serializes")
    }
}

/// Parse `MAJOR.MINOR.PATCH` with an optional `-prerelease` suffix
/// (the suffix is stripped, matching how Cargo normalizes `2.0.0-dev`).
fn parse_semver(raw: &str) -> VersionResult<(u32, u32, u32)> {
    let core = raw.split(['-', '+']).next().unwrap_or(raw);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(VersionError::InvalidSemver { raw: raw.to_string() });
    }
    let mut nums = [0u32; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.parse::<u32>().map_err(|_| VersionError::InvalidSemver {
            raw: raw.to_string(),
        })?;
    }
    Ok((nums[0], nums[1], nums[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_metadata_is_valid() {
        let m = VersionMetadata::v2_0_0();
        m.validate().unwrap();
        assert_eq!(m.four_part_file_version().unwrap(), [2, 0, 0, 0]);
    }

    #[test]
    fn from_semver_derives_four_part_version() {
        let m = VersionMetadata::from_semver("2.1.3", "ACME").unwrap();
        assert_eq!(m.file_version, "2.1.3.0");
        assert_eq!(m.product_version, "2.1.3");
        m.validate().unwrap();
    }

    #[test]
    fn prerelease_suffix_is_accepted_and_normalized() {
        let m = VersionMetadata::from_semver("2.0.0-dev", "ACME").unwrap();
        assert_eq!(m.product_version, "2.0.0");
    }

    #[test]
    fn invalid_versions_are_rejected() {
        assert!(matches!(
            VersionMetadata::from_semver("2.0", "ACME"),
            Err(VersionError::InvalidSemver { .. })
        ));
        assert!(matches!(
            VersionMetadata::from_semver("a.b.c", "ACME"),
            Err(VersionError::InvalidSemver { .. })
        ));
    }

    #[test]
    fn empty_company_is_rejected() {
        let mut m = VersionMetadata::v2_0_0();
        m.company = "   ".to_string();
        assert!(matches!(
            m.validate(),
            Err(VersionError::EmptyField { field: "company" })
        ));
    }

    #[test]
    fn json_roundtrips() {
        let m = VersionMetadata::v2_0_0();
        let back: VersionMetadata = serde_json::from_str(&m.to_json()).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn tauri_snippet_contains_required_keys() {
        let snippet = VersionMetadata::v2_0_0().render_tauri_conf_snippet();
        let v: serde_json::Value = serde_json::from_str(&snippet).unwrap();
        assert_eq!(v["productName"], "ModernToDoList");
        assert_eq!(v["version"], "2.0.0");
        assert_eq!(v["bundle"]["windows"]["publisher"], "ModernToDoList Team");
        assert_eq!(v["bundle"]["windows"]["fileVersion"], "2.0.0.0");
    }
}
