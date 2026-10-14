//! Dependency license collection (RD-M10-037 / INH-1123).
//!
//! Generates the `LICENSES/` notice files from the two lockfiles:
//! - Rust crates: parsed from `src-tauri/Cargo.lock`.
//! - npm packages: parsed from `package-lock.json` (lockfileVersion 3).
//!
//! Neither lockfile stores SPDX license identifiers for every entry, so this
//! module ships a curated table of the licenses for all *direct* dependencies
//! (verified against crates.io / npmjs at 2.0.0 GA time). Transitive
//! dependencies are listed with their source URL so the exact license can be
//! resolved mechanically; npm entries include the `license` field whenever the
//! lockfile recorded one.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LicenseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Malformed Cargo.lock: {0}")]
    MalformedCargoLock(String),

    #[error("Malformed package-lock.json: {0}")]
    MalformedPackageLock(String),
}

pub type LicenseResult<T> = Result<T, LicenseError>;

/// One Rust crate entry from `Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RustCrateDep {
    pub name: String,
    pub version: String,
    /// Registry URL when present; `None` for path (first-party) packages.
    pub source: Option<String>,
    /// SPDX expression from the curated table, or a resolver URL.
    pub license: String,
}

/// One npm package entry from `package-lock.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmDep {
    pub name: String,
    pub version: String,
    pub dev: bool,
    /// `license` field from the lockfile when recorded, else resolver URL.
    pub license: String,
}

/// Full third-party dependency inventory for the release.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInventory {
    pub rust_crates: Vec<RustCrateDep>,
    pub npm_packages: Vec<NpmDep>,
}

/// Curated SPDX licenses for the direct Rust dependencies declared in
/// `src-tauri/Cargo.toml` (2.0.0 GA dependency set).
fn known_rust_licenses() -> BTreeMap<&'static str, &'static str> {
    [
        ("blake3", "Apache-2.0 OR CC0-1.0"),
        ("chrono", "MIT OR Apache-2.0"),
        ("env_logger", "MIT OR Apache-2.0"),
        ("log", "MIT OR Apache-2.0"),
        ("notify", "CC0-1.0"),
        ("quick-xml", "MIT"),
        ("rand", "MIT OR Apache-2.0"),
        ("regex", "MIT OR Apache-2.0"),
        ("rusqlite", "MIT"),
        ("scraper", "MIT"),
        ("serde", "MIT OR Apache-2.0"),
        ("serde_json", "MIT OR Apache-2.0"),
        ("sha2", "MIT OR Apache-2.0"),
        ("tauri", "MIT OR Apache-2.0"),
        ("tauri-build", "MIT OR Apache-2.0"),
        ("tauri-plugin-shell", "MIT OR Apache-2.0"),
        ("tauri-plugin-single-instance", "MIT OR Apache-2.0"),
        ("tempfile", "MIT OR Apache-2.0"),
        ("thiserror", "MIT OR Apache-2.0"),
        ("tokio", "MIT"),
        ("uuid", "Apache-2.0 OR MIT"),
        ("walkdir", "Unlicense OR MIT"),
        ("zip", "MIT"),
    ]
    .into_iter()
    .collect()
}

fn license_fallback(kind: &str, name: &str) -> String {
    match kind {
        "rust" => format!("See https://crates.io/crates/{name} (transitive dependency)"),
        _ => format!("See https://www.npmjs.com/package/{name} (transitive dependency)"),
    }
}

/// Parse a `Cargo.lock` document into crate entries (sorted by name, then version).
pub fn parse_cargo_lock(contents: &str) -> LicenseResult<Vec<RustCrateDep>> {
    let known = known_rust_licenses();
    let mut out = Vec::new();
    let mut in_pkg = false;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut source: Option<String> = None;

    let flush = |out: &mut Vec<RustCrateDep>,
                     name: &mut Option<String>,
                     version: &mut Option<String>,
                     source: &mut Option<String>| {
        if let (Some(n), Some(v)) = (name.take(), version.take()) {
            let src = source.take();
            // Skip first-party path packages (no `source` key), but keep
            // everything pulled from a registry.
            if src.is_some() {
                let license = known
                    .get(n.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| license_fallback("rust", &n));
                out.push(RustCrateDep {
                    name: n,
                    version: v,
                    source: src,
                    license,
                });
            } else {
                source.take();
            }
        } else {
            *name = None;
            *version = None;
            *source = None;
        }
    };

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            flush(&mut out, &mut name, &mut version, &mut source);
            in_pkg = true;
            continue;
        }
        if trimmed.starts_with('[') {
            flush(&mut out, &mut name, &mut version, &mut source);
            in_pkg = false;
            continue;
        }
        if !in_pkg {
            continue;
        }
        if let Some(v) = trimmed.strip_prefix("name = ") {
            name = Some(unquote(v));
        } else if let Some(v) = trimmed.strip_prefix("version = ") {
            version = Some(unquote(v));
        } else if let Some(v) = trimmed.strip_prefix("source = ") {
            source = Some(unquote(v));
        }
    }
    flush(&mut out, &mut name, &mut version, &mut source);

    if out.is_empty() {
        return Err(LicenseError::MalformedCargoLock(
            "no [[package]] entries with a registry source found".to_string(),
        ));
    }
    out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(out)
}

fn unquote(raw: &str) -> String {
    raw.trim().trim_matches('"').to_string()
}

/// Parse a lockfileVersion 3 `package-lock.json` into npm entries.
pub fn parse_package_lock(contents: &str) -> LicenseResult<Vec<NpmDep>> {
    let v: serde_json::Value = serde_json::from_str(contents)
        .map_err(|e| LicenseError::MalformedPackageLock(e.to_string()))?;
    let packages = v["packages"]
        .as_object()
        .ok_or_else(|| LicenseError::MalformedPackageLock("missing 'packages' object".into()))?;

    let mut out = Vec::new();
    for (key, meta) in packages {
        // Keys look like "node_modules/<name>" or "" for the root project.
        let name = match key.rsplit_once("node_modules/") {
            Some((_, n)) if !n.is_empty() => n.to_string(),
            _ => continue, // root project or non-package entry
        };
        let version = meta["version"].as_str().unwrap_or("unknown").to_string();
        let dev = meta["dev"].as_bool().unwrap_or(false);
        let license = meta["license"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| license_fallback("npm", &name));
        out.push(NpmDep {
            name,
            version,
            dev,
            license,
        });
    }
    if out.is_empty() {
        return Err(LicenseError::MalformedPackageLock(
            "no node_modules entries found".into(),
        ));
    }
    out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(out)
}

impl LicenseInventory {
    /// Build the inventory from the two lockfiles on disk.
    pub fn from_lockfiles(cargo_lock: &Path, package_lock: &Path) -> LicenseResult<Self> {
        let rust_crates = parse_cargo_lock(&std::fs::read_to_string(cargo_lock)?)?;
        let npm_packages = parse_package_lock(&std::fs::read_to_string(package_lock)?)?;
        Ok(Self {
            rust_crates,
            npm_packages,
        })
    }

    pub fn render_rust_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Rust crate dependencies (from src-tauri/Cargo.lock)\n\n");
        md.push_str("ModernToDoList 2.0.0 statically links the following third-party crates.\n");
        md.push_str("Licenses for direct dependencies are listed explicitly; transitive\n");
        md.push_str("dependencies link to their crates.io page where the license is authoritative.\n\n");
        md.push_str("| Crate | Version | License |\n|---|---|---|\n");
        for c in &self.rust_crates {
            md.push_str(&format!("| {} | {} | {} |\n", c.name, c.version, c.license));
        }
        md
    }

    pub fn render_npm_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# npm package dependencies (from package-lock.json)\n\n");
        md.push_str("These packages are used to build the bundled frontend assets.\n");
        md.push_str("`dev` marks build-time-only dependencies that are not shipped in the ZIP.\n\n");
        md.push_str("| Package | Version | Kind | License |\n|---|---|---|---|\n");
        for p in &self.npm_packages {
            let kind = if p.dev { "dev" } else { "runtime" };
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                p.name, p.version, kind, p.license
            ));
        }
        md
    }

    pub fn render_notice(&self) -> String {
        format!(
            "# Third-Party Notices — ModernToDoList 2.0.0\n\n\
This product includes software developed by third parties.\n\
Full inventories: `RUST_DEPENDENCIES.md` ({} crates) and `NPM_DEPENDENCIES.md` ({} packages).\n\n\
The application itself is distributed under the terms in `LICENSE.txt`.\n\
Where a component is dual-licensed (e.g. `MIT OR Apache-2.0`), the recipient\n\
may choose either license. Copies of the MIT and Apache-2.0 texts are\n\
available at https://opensource.org/licenses/MIT and\n\
https://www.apache.org/licenses/LICENSE-2.0 respectively.\n",
            self.rust_crates.len(),
            self.npm_packages.len()
        )
    }

    /// Write `RUST_DEPENDENCIES.md`, `NPM_DEPENDENCIES.md` and
    /// `THIRD_PARTY_NOTICES.md` into `out_dir`. Deterministic output:
    /// identical lockfiles always produce identical files.
    pub fn write_license_files(&self, out_dir: &Path) -> LicenseResult<()> {
        std::fs::create_dir_all(out_dir)?;
        std::fs::write(
            out_dir.join("RUST_DEPENDENCIES.md"),
            self.render_rust_markdown(),
        )?;
        std::fs::write(out_dir.join("NPM_DEPENDENCIES.md"), self.render_npm_markdown())?;
        std::fs::write(out_dir.join("THIRD_PARTY_NOTICES.md"), self.render_notice())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOCK: &str = r#"
version = 3

[[package]]
name = "moderntodolist"
version = "2.0.0-dev"

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "some-transitive"
version = "0.3.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    const SAMPLE_NPM_LOCK: &str = r#"{
  "name": "moderntodolist",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "moderntodolist", "version": "2.0.0-dev" },
    "node_modules/vue": { "version": "3.5.13", "license": "MIT" },
    "node_modules/typescript": { "version": "5.6.3", "dev": true, "license": "Apache-2.0" },
    "node_modules/@vitejs/plugin-vue": { "version": "5.2.1", "dev": true }
  }
}"#;

    #[test]
    fn cargo_lock_parsing_skips_first_party_and_sorts() {
        let deps = parse_cargo_lock(SAMPLE_LOCK).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "serde");
        assert_eq!(deps[0].license, "MIT OR Apache-2.0");
        assert!(deps[1].license.contains("crates.io/crates/some-transitive"));
    }

    #[test]
    fn malformed_cargo_lock_is_rejected() {
        assert!(matches!(
            parse_cargo_lock("not a lockfile"),
            Err(LicenseError::MalformedCargoLock(_))
        ));
    }

    #[test]
    fn npm_lock_parsing_records_license_and_dev_flag() {
        let deps = parse_package_lock(SAMPLE_NPM_LOCK).unwrap();
        assert_eq!(deps.len(), 3);
        let vue = deps.iter().find(|d| d.name == "vue").unwrap();
        assert_eq!(vue.license, "MIT");
        assert!(!vue.dev);
        let ts = deps.iter().find(|d| d.name == "typescript").unwrap();
        assert!(ts.dev);
        let plugin = deps
            .iter()
            .find(|d| d.name == "@vitejs/plugin-vue")
            .unwrap();
        assert!(plugin.license.contains("npmjs.com"));
    }

    #[test]
    fn malformed_npm_lock_is_rejected() {
        assert!(matches!(
            parse_package_lock("{}"),
            Err(LicenseError::MalformedPackageLock(_))
        ));
    }

    #[test]
    fn markdown_tables_render_all_entries() {
        let inv = LicenseInventory {
            rust_crates: parse_cargo_lock(SAMPLE_LOCK).unwrap(),
            npm_packages: parse_package_lock(SAMPLE_NPM_LOCK).unwrap(),
        };
        let rust_md = inv.render_rust_markdown();
        assert!(rust_md.contains("| serde | 1.0.219 | MIT OR Apache-2.0 |"));
        let npm_md = inv.render_npm_markdown();
        assert!(npm_md.contains("| vue | 3.5.13 | runtime | MIT |"));
        assert!(inv.render_notice().contains("2 crates"));
    }

    #[test]
    fn write_license_files_creates_three_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let inv = LicenseInventory {
            rust_crates: parse_cargo_lock(SAMPLE_LOCK).unwrap(),
            npm_packages: parse_package_lock(SAMPLE_NPM_LOCK).unwrap(),
        };
        inv.write_license_files(tmp.path()).unwrap();
        for f in [
            "RUST_DEPENDENCIES.md",
            "NPM_DEPENDENCIES.md",
            "THIRD_PARTY_NOTICES.md",
        ] {
            assert!(tmp.path().join(f).exists(), "{f} should exist");
        }
    }
}
