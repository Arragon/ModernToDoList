//! Release engineering for ModernToDoList 2.0 (RD-M10-024, RD-M10-037~040).
//!
//! Submodules:
//! - [`packaging`]: deterministic (reproducible) Portable ZIP generation with SHA-256.
//! - [`version_metadata`]: file/product version metadata embedded into the EXE.
//! - [`licenses`]: dependency license collection from `Cargo.lock` and `package-lock.json`.
//! - [`notes`]: release-notes template and the 2.0.0 changelog.

pub mod licenses;
pub mod notes;
pub mod packaging;
pub mod version_metadata;

#[allow(unused_imports)]
pub use packaging::{
    build_deterministic_zip, build_portable_zip_from_dir, portable_zip_name, sha256_hex, ZipEntrySpec,
};
#[allow(unused_imports)]
pub use version_metadata::{VersionError, VersionMetadata};
