//! Deterministic release ZIP packaging (RD-M10-024 / INH-1117).
//!
//! The Portable release archive must be *reproducible*: building the ZIP twice
//! from the same inputs must yield byte-identical output (and therefore the
//! same SHA-256). This module guarantees that by:
//!
//! 1. Sorting all entries by their archive path (byte order, `/` separators).
//! 2. Stamping every entry with a fixed modification timestamp
//!    (2025-01-01 00:00:00, well inside the DOS/ZIP epoch range).
//! 3. Using one consistent compression configuration for every entry
//!    (Deflate, explicit level 9) and for directories (Stored).
//! 4. Never embedding build-machine metadata (no extended-timestamp or
//!    UID/GID extra fields — `zip` is used with `default-features = false`).
//!
//! The canonical release artifact name is
//! `ModernToDoList-<version>-Portable-win-x64.zip`, accompanied by a
//! `.sha256` sidecar file.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Fixed timestamp applied to every ZIP entry so that archives are reproducible.
pub const FIXED_YEAR: u16 = 2025;
pub const FIXED_MONTH: u8 = 1;
pub const FIXED_DAY: u8 = 1;
pub const FIXED_HOUR: u8 = 0;
pub const FIXED_MIN: u8 = 0;
pub const FIXED_SEC: u8 = 0;

/// Explicit, consistent compression settings.
pub const COMPRESSION_LEVEL: i64 = 9;

#[derive(Debug, Error)]
pub enum PackagingError {
    #[error("ZIP writer error: {0}")]
    Zip(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid archive entry name: {0}")]
    InvalidEntryName(String),

    #[error("Staging directory does not exist: {0}")]
    StagingMissing(PathBuf),
}

impl From<zip::result::ZipError> for PackagingError {
    fn from(e: zip::result::ZipError) -> Self {
        PackagingError::Zip(e.to_string())
    }
}

pub type PackagingResult<T> = Result<T, PackagingError>;

/// One logical entry of the release archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipEntrySpec {
    /// Archive path, relative, using `/` separators (e.g. `ModernToDoList.exe`).
    pub path: String,
    /// File contents. Empty for directories.
    pub contents: Vec<u8>,
    /// True if this entry is a directory (name must end with `/`).
    pub is_dir: bool,
}

impl ZipEntrySpec {
    pub fn file(path: impl Into<String>, contents: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
            is_dir: false,
        }
    }

    pub fn dir(path: impl Into<String>) -> Self {
        let mut p = path.into();
        if !p.ends_with('/') {
            p.push('/');
        }
        Self {
            path: p,
            contents: Vec::new(),
            is_dir: true,
        }
    }
}

/// Canonical Portable release artifact name for a version, e.g.
/// `ModernToDoList-2.0.0-Portable-win-x64.zip`.
pub fn portable_zip_name(version: &str) -> String {
    format!("ModernToDoList-{version}-Portable-win-x64.zip")
}

/// Compute the SHA-256 of a byte slice, as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn fixed_timestamp() -> PackagingResult<zip::DateTime> {
    zip::DateTime::from_date_and_time(FIXED_YEAR, FIXED_MONTH, FIXED_DAY, FIXED_HOUR, FIXED_MIN, FIXED_SEC)
        .map_err(|e| PackagingError::Zip(format!("fixed timestamp rejected: {e}")))
}

fn file_options() -> PackagingResult<SimpleFileOptions> {
    Ok(SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(COMPRESSION_LEVEL))
        .last_modified_time(fixed_timestamp()?))
}

fn dir_options() -> PackagingResult<SimpleFileOptions> {
    Ok(SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(fixed_timestamp()?))
}

/// Normalize an archive path: `/` separators, no leading `/` or `./`.
fn normalize_entry_path(raw: &str) -> PackagingResult<String> {
    let unified = raw.replace('\\', "/");
    let trimmed = unified
        .trim_start_matches('/')
        .trim_start_matches("./")
        .to_string();
    if trimmed.is_empty() || trimmed.contains("..") {
        return Err(PackagingError::InvalidEntryName(raw.to_string()));
    }
    Ok(trimmed)
}

/// Build a deterministic ZIP archive in memory from the given entries.
///
/// Entries are sorted by normalized archive path; duplicates are rejected.
/// The same input always produces byte-identical output.
pub fn build_deterministic_zip(entries: &[ZipEntrySpec]) -> PackagingResult<Vec<u8>> {
    // Normalize + sort for deterministic ordering.
    let mut normalized: Vec<ZipEntrySpec> = Vec::with_capacity(entries.len());
    for e in entries {
        let path = normalize_entry_path(&e.path)?;
        let path = if e.is_dir && !path.ends_with('/') {
            format!("{path}/")
        } else {
            path
        };
        normalized.push(ZipEntrySpec { path, ..e.clone() });
    }
    normalized.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    // Reject duplicates: they would make output depend on input order.
    for w in normalized.windows(2) {
        if w[0].path == w[1].path {
            return Err(PackagingError::InvalidEntryName(format!(
                "duplicate entry '{}'",
                w[0].path
            )));
        }
    }

    let writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut zip = writer;

    for entry in &normalized {
        if entry.is_dir {
            zip.add_directory(entry.path.clone(), dir_options()?)?;
        } else {
            zip.start_file(entry.path.clone(), file_options()?)?;
            zip.write_all(&entry.contents)?;
        }
    }

    let cursor = zip.finish()?;
    Ok(cursor.into_inner())
}

/// Collect every file (and directory) under `staging_dir` into deterministic
/// `ZipEntrySpec`s with archive paths relative to the staging root.
pub fn collect_staging_entries(staging_dir: &Path) -> PackagingResult<Vec<ZipEntrySpec>> {
    if !staging_dir.is_dir() {
        return Err(PackagingError::StagingMissing(staging_dir.to_path_buf()));
    }

    let mut specs = Vec::new();
    for item in WalkDir::new(staging_dir).min_depth(1).sort_by_file_name() {
        let item = item.map_err(|e| PackagingError::Io(e.into()))?;
        let rel = item
            .path()
            .strip_prefix(staging_dir)
            .map_err(|e| PackagingError::InvalidEntryName(e.to_string()))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if item.file_type().is_dir() {
            specs.push(ZipEntrySpec::dir(rel_str));
        } else if item.file_type().is_file() {
            let bytes = std::fs::read(item.path())?;
            specs.push(ZipEntrySpec::file(rel_str, bytes));
        }
        // Symlinks are intentionally skipped for reproducibility.
    }
    Ok(specs)
}

/// Build the release ZIP from a staging directory and write it to `out_dir`.
///
/// Writes two files:
/// - `<out_dir>/ModernToDoList-<version>-Portable-win-x64.zip`
/// - `<out_dir>/ModernToDoList-<version>-Portable-win-x64.zip.sha256`
///
/// Returns `(zip_path, sha256_hex)`.
pub fn build_portable_zip_from_dir(
    staging_dir: &Path,
    out_dir: &Path,
    version: &str,
) -> PackagingResult<(PathBuf, String)> {
    let specs = collect_staging_entries(staging_dir)?;
    let bytes = build_deterministic_zip(&specs)?;
    let hash = sha256_hex(&bytes);

    std::fs::create_dir_all(out_dir)?;
    let name = portable_zip_name(version);
    let zip_path = out_dir.join(&name);
    std::fs::write(&zip_path, &bytes)?;
    std::fs::write(out_dir.join(format!("{name}.sha256")), format!("{hash}  {name}\n"))?;

    Ok((zip_path, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<ZipEntrySpec> {
        vec![
            ZipEntrySpec::file("ModernToDoList.exe", b"fake-exe-bytes".to_vec()),
            ZipEntrySpec::file("Data/settings.json", br#"{"schemaVersion":2}"#.to_vec()),
            ZipEntrySpec::dir("Data"),
            ZipEntrySpec::file("readme.txt", "portable app\r\n".as_bytes().to_vec()),
        ]
    }

    #[test]
    fn zip_is_byte_identical_across_rebuilds() {
        let a = build_deterministic_zip(&sample_entries()).unwrap();
        let b = build_deterministic_zip(&sample_entries()).unwrap();
        assert_eq!(a, b, "archive bytes must be reproducible");
        assert_eq!(sha256_hex(&a), sha256_hex(&b));
        assert_eq!(sha256_hex(&a).len(), 64);
    }

    #[test]
    fn zip_is_independent_of_input_order() {
        let mut shuffled = sample_entries();
        shuffled.reverse();
        let a = build_deterministic_zip(&sample_entries()).unwrap();
        let b = build_deterministic_zip(&shuffled).unwrap();
        assert_eq!(sha256_hex(&a), sha256_hex(&b));
    }

    #[test]
    fn duplicate_entries_are_rejected() {
        let dup = vec![
            ZipEntrySpec::file("a.txt", b"1".to_vec()),
            ZipEntrySpec::file("a.txt", b"2".to_vec()),
        ];
        assert!(matches!(
            build_deterministic_zip(&dup),
            Err(PackagingError::InvalidEntryName(_))
        ));
    }

    #[test]
    fn traversal_entry_names_are_rejected() {
        let bad = vec![ZipEntrySpec::file("../escape.txt", b"x".to_vec())];
        assert!(matches!(
            build_deterministic_zip(&bad),
            Err(PackagingError::InvalidEntryName(_))
        ));
    }

    #[test]
    fn archive_roundtrips_content_and_paths() {
        let bytes = build_deterministic_zip(&sample_entries()).unwrap();
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes)).expect("readable archive");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"ModernToDoList.exe".to_string()));
        assert!(names.contains(&"Data/settings.json".to_string()));

        let mut exe = archive.by_name("ModernToDoList.exe").unwrap();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut exe, &mut buf).unwrap();
        assert_eq!(buf, b"fake-exe-bytes");
    }

    #[test]
    fn portable_zip_name_matches_release_convention() {
        assert_eq!(
            portable_zip_name("2.0.0"),
            "ModernToDoList-2.0.0-Portable-win-x64.zip"
        );
    }

    #[test]
    fn build_from_staging_dir_writes_zip_and_sha256_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(staging.join("Data")).unwrap();
        std::fs::write(staging.join("ModernToDoList.exe"), b"exe").unwrap();
        std::fs::write(staging.join("Data").join("settings.json"), b"{}").unwrap();

        let out = tmp.path().join("out");
        let (zip_path, hash1) =
            build_portable_zip_from_dir(&staging, &out, "2.0.0").unwrap();
        assert_eq!(
            zip_path.file_name().unwrap().to_string_lossy(),
            "ModernToDoList-2.0.0-Portable-win-x64.zip"
        );

        // Rebuild and compare hashes: deterministic end-to-end.
        let (_, hash2) = build_portable_zip_from_dir(&staging, &out, "2.0.0").unwrap();
        assert_eq!(hash1, hash2);

        let sidecar = std::fs::read_to_string(out.join(format!(
            "ModernToDoList-2.0.0-Portable-win-x64.zip.sha256"
        )))
        .unwrap();
        assert!(sidecar.starts_with(&hash1));
    }
}
