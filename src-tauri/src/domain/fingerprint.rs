//! File fingerprinting using BLAKE3 hashing.
//!
//! Provides content-addressable file identification for change detection,
//! conflict resolution, and data safety verification.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A content fingerprint for a file, computed using BLAKE3.
///
/// Used to detect external modifications, verify save integrity,
/// and identify files without relying on timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileFingerprint {
    /// BLAKE3 hash as a hex string (64 chars for 256-bit hash).
    pub hash: String,
    /// File size in bytes at the time of hashing.
    pub size: u64,
}

impl FileFingerprint {
    /// Computes the BLAKE3 fingerprint of a file.
    pub fn from_file(path: &Path) -> Result<Self, FingerprintError> {
        let data = std::fs::read(path)
            .map_err(|e| FingerprintError::IoError(e.to_string()))?;
        Ok(Self::from_bytes(&data))
    }

    /// Computes the BLAKE3 fingerprint of a byte slice.
    pub fn from_bytes(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Self {
            hash: hash.to_hex().to_string(),
            size: data.len() as u64,
        }
    }

    /// Computes a streaming BLAKE3 hash for large files.
    pub fn from_reader(mut reader: impl std::io::Read) -> Result<Self, FingerprintError> {
        let mut hasher = blake3::Hasher::new();
        let mut buf = [0u8; 8192];
        let mut total_size: u64 = 0;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| FingerprintError::IoError(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total_size += n as u64;
        }
        let hash = hasher.finalize();
        Ok(Self {
            hash: hash.to_hex().to_string(),
            size: total_size,
        })
    }

    /// Returns true if this fingerprint matches the given data.
    pub fn matches_bytes(&self, data: &[u8]) -> bool {
        let hash = blake3::hash(data);
        self.hash == hash.to_hex().to_string()
    }

    /// Creates a zero/empty fingerprint (for new documents).
    pub fn empty() -> Self {
        Self {
            hash: blake3::hash(b"").to_hex().to_string(),
            size: 0,
        }
    }
}

/// Errors that can occur during fingerprinting.
#[derive(Debug, Clone)]
pub enum FingerprintError {
    IoError(String),
}

impl std::fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FingerprintError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for FingerprintError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_from_bytes() {
        let fp = FileFingerprint::from_bytes(b"hello world");
        assert_eq!(fp.size, 11);
        assert_eq!(fp.hash.len(), 64); // BLAKE3 produces 256-bit (32-byte) hash = 64 hex chars
    }

    #[test]
    fn fingerprint_deterministic() {
        let fp1 = FileFingerprint::from_bytes(b"test data");
        let fp2 = FileFingerprint::from_bytes(b"test data");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_different_data() {
        let fp1 = FileFingerprint::from_bytes(b"data A");
        let fp2 = FileFingerprint::from_bytes(b"data B");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn fingerprint_matches_bytes() {
        let data = b"hello world";
        let fp = FileFingerprint::from_bytes(data);
        assert!(fp.matches_bytes(data));
        assert!(!fp.matches_bytes(b"different"));
    }

    #[test]
    fn fingerprint_from_reader() {
        let data = b"streaming test data";
        let fp = FileFingerprint::from_reader(&data[..]).unwrap();
        let fp_direct = FileFingerprint::from_bytes(data);
        assert_eq!(fp, fp_direct);
    }

    #[test]
    fn fingerprint_empty() {
        let fp = FileFingerprint::empty();
        assert_eq!(fp.size, 0);
        assert!(fp.matches_bytes(b""));
    }

    #[test]
    fn fingerprint_serde_roundtrip() {
        let fp = FileFingerprint::from_bytes(b"test");
        let json = serde_json::to_string(&fp).unwrap();
        let decoded: FileFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, decoded);
    }
}
