//! Canonical performance datasets (RD-M10-002..009, INH-1111).
//!
//! All datasets are produced by the deterministic fixture generator
//! ([`super::fixture_gen`]) from fixed seeds, so any machine can regenerate
//! byte-identical inputs. The catalogue covers:
//!
//! - Scale variants: `scale-1k`, `scale-10k`, `scale-50k`, `scale-100k`.
//! - Shape variants: `shape-deep-tree` (nesting depth >= 20),
//!   `shape-rich-text`, `shape-attachment-heavy`, `shape-dependency-heavy`.
//!
//! Large scales are too slow for unit tests, so the *active* set is gated by
//! the `MTL_BENCH_SCALE` environment variable (default `small` = the 1k
//! dataset only). [`generate_dataset`] / [`dataset_by_name`] can still produce
//! any scale on demand, regardless of the gate.

use std::fs;
use std::path::Path;

use super::fixture_gen::{
    generate_file, DatasetShape, FixtureConfig, FixtureManifest, ManifestEntry,
};

/// Environment variable selecting which datasets are "active".
pub const SCALE_ENV_VAR: &str = "MTL_BENCH_SCALE";

/// Environment variable overriding every dataset seed (reproducibility knob
/// for `scripts/bench/run-benchmark.cjs`).
pub const SEED_ENV_VAR: &str = "MTL_BENCH_SEED";

/// Environment variable pointing at the benchmark output directory.
pub const OUT_ENV_VAR: &str = "MTL_BENCH_OUT";

/// Which datasets a benchmark run should include.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFilter {
    /// Default: only `scale-1k` (fast enough for unit tests).
    Small,
    /// Only the four shape variants (1k tasks each).
    Shapes,
    /// `scale-1k` + `scale-10k`.
    Medium,
    /// `scale-1k` + `scale-10k` + `scale-50k`.
    Large,
    /// Everything, including `scale-100k`.
    All,
}

/// Parses the `MTL_BENCH_SCALE` value; unknown/absent falls back to `Small`.
pub fn parse_scale_filter(value: Option<&str>) -> ScaleFilter {
    match value.map(|v| v.trim().to_lowercase()) {
        None => ScaleFilter::Small,
        Some(ref v) if v.is_empty() => ScaleFilter::Small,
        Some(ref v) => match v.as_str() {
            "shapes" | "shape" => ScaleFilter::Shapes,
            "medium" | "10k" => ScaleFilter::Medium,
            "large" | "50k" => ScaleFilter::Large,
            "all" | "xlarge" | "100k" => ScaleFilter::All,
            _ => ScaleFilter::Small,
        },
    }
}

/// One canonical dataset: a name plus the exact generator configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct DatasetSpec {
    /// Stable dataset identifier (also used as directory/file name).
    pub name: String,
    /// Generator configuration.
    pub config: FixtureConfig,
}

impl DatasetSpec {
    /// Expected number of tasks in the generated file.
    pub fn task_count(&self) -> usize {
        self.config.task_count
    }
}

/// Static catalogue entry.
struct DatasetDef {
    name: &'static str,
    shape: DatasetShape,
    task_count: usize,
    seed: u64,
}

/// The full canonical catalogue (fixed seeds => byte-reproducible datasets).
const CANONICAL: &[DatasetDef] = &[
    DatasetDef { name: "scale-1k", shape: DatasetShape::Scale, task_count: 1_000, seed: 0x4D54_0000_0000_0001 },
    DatasetDef { name: "scale-10k", shape: DatasetShape::Scale, task_count: 10_000, seed: 0x4D54_0000_0000_0002 },
    DatasetDef { name: "scale-50k", shape: DatasetShape::Scale, task_count: 50_000, seed: 0x4D54_0000_0000_0003 },
    DatasetDef { name: "scale-100k", shape: DatasetShape::Scale, task_count: 100_000, seed: 0x4D54_0000_0000_0004 },
    DatasetDef { name: "shape-deep-tree", shape: DatasetShape::DeepTree, task_count: 1_000, seed: 0x4D54_0000_0000_0101 },
    DatasetDef { name: "shape-rich-text", shape: DatasetShape::RichText, task_count: 1_000, seed: 0x4D54_0000_0000_0102 },
    DatasetDef { name: "shape-attachment-heavy", shape: DatasetShape::AttachmentHeavy, task_count: 1_000, seed: 0x4D54_0000_0000_0103 },
    DatasetDef { name: "shape-dependency-heavy", shape: DatasetShape::DependencyHeavy, task_count: 1_000, seed: 0x4D54_0000_0000_0104 },
];

fn spec_for(def: &DatasetDef, seed_override: Option<u64>) -> DatasetSpec {
    let config = FixtureConfig::for_shape(def.shape, def.task_count, def.seed)
        .with_name(def.name)
        .with_seed(seed_override.unwrap_or(def.seed));
    DatasetSpec {
        name: def.name.to_string(),
        config,
    }
}

/// Returns the full canonical catalogue (all scales and shapes).
///
/// `seed_override` replaces every fixed seed (useful for variance studies);
/// pass `None` for the canonical reproducible seeds.
pub fn canonical_datasets(seed_override: Option<u64>) -> Vec<DatasetSpec> {
    CANONICAL.iter().map(|d| spec_for(d, seed_override)).collect()
}

/// Looks up one canonical dataset by name, at any scale (never env-gated).
pub fn dataset_by_name(name: &str, seed_override: Option<u64>) -> Option<DatasetSpec> {
    CANONICAL
        .iter()
        .find(|d| d.name == name)
        .map(|d| spec_for(d, seed_override))
}

/// True if `spec` passes the given scale filter.
pub fn filter_allows(filter: ScaleFilter, spec: &DatasetSpec) -> bool {
    let is_shape = spec.name.starts_with("shape-");
    match filter {
        ScaleFilter::Small => spec.name == "scale-1k",
        ScaleFilter::Shapes => is_shape,
        ScaleFilter::Medium => spec.name == "scale-1k" || spec.name == "scale-10k",
        ScaleFilter::Large => {
            spec.name == "scale-1k" || spec.name == "scale-10k" || spec.name == "scale-50k"
        }
        ScaleFilter::All => true,
    }
}

/// Returns the catalogue filtered by `filter`.
pub fn specs_for_filter(filter: ScaleFilter, seed_override: Option<u64>) -> Vec<DatasetSpec> {
    canonical_datasets(seed_override)
        .into_iter()
        .filter(|s| filter_allows(filter, s))
        .collect()
}

/// Returns the datasets active for this process, honoring `MTL_BENCH_SCALE`
/// (default: `small` = `scale-1k` only) and `MTL_BENCH_SEED`.
pub fn active_datasets() -> Vec<DatasetSpec> {
    let filter = parse_scale_filter(std::env::var(SCALE_ENV_VAR).ok().as_deref());
    let seed_override = std::env::var(SEED_ENV_VAR)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok());
    specs_for_filter(filter, seed_override)
}

/// Generates one dataset into `<dir>/<spec.name>/`, returning the manifest
/// entry (with the dataset-relative file path). Works at any scale and is
/// independent of the `MTL_BENCH_SCALE` gate.
pub fn generate_dataset(dir: &Path, spec: &DatasetSpec) -> std::io::Result<ManifestEntry> {
    let dataset_dir = dir.join(&spec.name);
    let mut entry = generate_file(&spec.config, &dataset_dir)?;
    entry.file = format!("{}/{}", spec.name, spec.config.filename);
    Ok(entry)
}

/// Generates a list of datasets under `dir` (one subdirectory each) and
/// writes `MANIFEST.json` + `MANIFEST.sha256` covering all of them.
pub fn generate_datasets(dir: &Path, specs: &[DatasetSpec]) -> std::io::Result<FixtureManifest> {
    fs::create_dir_all(dir)?;
    let mut manifest = FixtureManifest::new();
    for spec in specs {
        let entry = generate_dataset(dir, spec)?;
        manifest.push(entry, spec.config.clone());
    }
    manifest.write(dir)?;
    Ok(manifest)
}

/// Convenience: generate the datasets active for this process into `dir`.
pub fn generate_active_datasets(dir: &Path) -> std::io::Result<FixtureManifest> {
    let specs = active_datasets();
    generate_datasets(dir, &specs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::fixture_gen::{count_task_elements, max_task_depth};
    use crate::domain::parse_xml;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mtdl_benchds_{}_{}_{}",
            tag,
            std::process::id(),
            rand::random::<u32>()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn scale_filter_parsing() {
        assert_eq!(parse_scale_filter(None), ScaleFilter::Small);
        assert_eq!(parse_scale_filter(Some("")), ScaleFilter::Small);
        assert_eq!(parse_scale_filter(Some("bogus")), ScaleFilter::Small);
        assert_eq!(parse_scale_filter(Some("small")), ScaleFilter::Small);
        assert_eq!(parse_scale_filter(Some("SHAPES")), ScaleFilter::Shapes);
        assert_eq!(parse_scale_filter(Some("medium")), ScaleFilter::Medium);
        assert_eq!(parse_scale_filter(Some("large")), ScaleFilter::Large);
        assert_eq!(parse_scale_filter(Some("all")), ScaleFilter::All);
    }

    #[test]
    fn catalogue_is_complete() {
        let all = canonical_datasets(None);
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "scale-1k",
                "scale-10k",
                "scale-50k",
                "scale-100k",
                "shape-deep-tree",
                "shape-rich-text",
                "shape-attachment-heavy",
                "shape-dependency-heavy",
            ]
        );
        // Scale variants carry the right task counts.
        assert_eq!(all[0].task_count(), 1_000);
        assert_eq!(all[1].task_count(), 10_000);
        assert_eq!(all[2].task_count(), 50_000);
        assert_eq!(all[3].task_count(), 100_000);
        // Deep-tree config enforces depth >= 20.
        let deep = dataset_by_name("shape-deep-tree", None).unwrap();
        assert!(deep.config.max_depth >= 20);
        // Seeds are fixed and distinct.
        let seeds: Vec<u64> = canonical_datasets(None)
            .iter()
            .map(|s| s.config.seed)
            .collect();
        let mut uniq = seeds.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), seeds.len());
    }

    #[test]
    fn filter_membership() {
        let all = canonical_datasets(None);
        let by = |n: &str| all.iter().find(|s| s.name == n).unwrap();
        assert!(filter_allows(ScaleFilter::Small, by("scale-1k")));
        assert!(!filter_allows(ScaleFilter::Small, by("scale-10k")));
        assert!(!filter_allows(ScaleFilter::Small, by("shape-deep-tree")));
        assert!(filter_allows(ScaleFilter::Shapes, by("shape-rich-text")));
        assert!(!filter_allows(ScaleFilter::Shapes, by("scale-1k")));
        assert!(filter_allows(ScaleFilter::Medium, by("scale-10k")));
        assert!(!filter_allows(ScaleFilter::Medium, by("scale-50k")));
        assert!(filter_allows(ScaleFilter::Large, by("scale-50k")));
        assert!(!filter_allows(ScaleFilter::Large, by("scale-100k")));
        assert!(filter_allows(ScaleFilter::All, by("scale-100k")));
        assert_eq!(specs_for_filter(ScaleFilter::All, None).len(), 8);
        assert_eq!(specs_for_filter(ScaleFilter::Small, None).len(), 1);
    }

    #[test]
    fn seed_override_applies() {
        let spec = dataset_by_name("scale-1k", Some(987)).unwrap();
        assert_eq!(spec.config.seed, 987);
        let canonical = dataset_by_name("scale-1k", None).unwrap();
        assert_ne!(canonical.config.seed, 987);
    }

    #[test]
    fn generate_small_dataset_parses_with_real_parser() {
        let dir = temp_dir("gen");
        let spec = dataset_by_name("scale-1k", None).unwrap();
        let entry = generate_dataset(&dir, &spec).unwrap();
        assert_eq!(entry.file, "scale-1k/scale-1k.xml");

        let path = dir.join("scale-1k").join("scale-1k.xml");
        assert!(path.exists());
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, entry.bytes);

        let doc = parse_xml(&bytes).expect("dataset must parse with the real parser");
        assert_eq!(count_task_elements(&doc.root), 1_000);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_datasets_writes_verifiable_manifest() {
        let dir = temp_dir("manset");
        let specs: Vec<DatasetSpec> = vec![
            dataset_by_name("scale-1k", None).unwrap(),
            dataset_by_name("shape-deep-tree", None).unwrap(),
        ];
        let manifest = generate_datasets(&dir, &specs).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.verify(&dir).unwrap().is_empty());
        assert!(dir.join("MANIFEST.json").exists());
        assert!(dir.join("MANIFEST.sha256").exists());

        // Regenerating into a fresh directory yields identical hashes.
        let dir2 = temp_dir("manset2");
        let manifest2 = generate_datasets(&dir2, &specs).unwrap();
        assert_eq!(
            manifest.files.iter().map(|f| (&f.file, &f.sha256)).collect::<Vec<_>>(),
            manifest2.files.iter().map(|f| (&f.file, &f.sha256)).collect::<Vec<_>>(),
            "datasets must be byte-reproducible across runs"
        );

        // Deep-tree dataset really nests >= 20 levels.
        let deep = fs::read(dir.join("shape-deep-tree").join("shape-deep-tree.xml")).unwrap();
        let doc = parse_xml(&deep).unwrap();
        assert!(max_task_depth(&doc.root) >= 20);

        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&dir2).ok();
    }

    #[test]
    fn any_scale_available_on_demand() {
        // Config lookup works for large scales without generating them.
        let spec = dataset_by_name("scale-100k", None).unwrap();
        assert_eq!(spec.task_count(), 100_000);
        assert!(dataset_by_name("nonexistent", None).is_none());
    }
}
