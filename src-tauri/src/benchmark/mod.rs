//! M10 benchmark and profiling infrastructure (RD-M10-001 .. RD-M10-021).
//!
//! This module provides:
//!
//! - [`fixture_gen`]: a deterministic, seeded benchmark fixture generator that
//!   produces TDL XML task trees with configurable depth, breadth and field
//!   density, plus a SHA-256 manifest for reproducibility (RD-M10-001).
//! - [`datasets`]: the canonical performance dataset catalogue (scale variants
//!   1k/10k/50k/100k and shape variants deep-tree / rich-text / attachment /
//!   dependency heavy), gated by the `MTL_BENCH_SCALE` environment variable
//!   (RD-M10-002..009).
//! - [`results`]: the versioned benchmark result format (JSON schema + Markdown
//!   renderer) (RD-M10-010).
//! - [`profile`]: the core profiling harness measuring startup, workspace
//!   scan, XML parse throughput, index rebuild and search latency against the
//!   real code paths, plus Windows process memory sampling via hand-written
//!   WinAPI FFI (RD-M10-011..021).
//!
//! See `docs/performance/BENCHMARK_FORMAT.md` and
//! `docs/performance/PROFILING_METHOD.md` for the on-disk formats and the
//! frontend Performance API mark contract.

pub mod datasets;
pub mod fixture_gen;
pub mod profile;
pub mod results;

pub use datasets::{
    active_datasets, canonical_datasets, dataset_by_name, generate_dataset, generate_datasets,
    parse_scale_filter, DatasetSpec, ScaleFilter,
};
pub use fixture_gen::{
    count_task_elements, generate_file, generate_xml, max_task_depth, sha256_hex, DatasetShape,
    FixtureConfig, FixtureManifest, ManifestEntry, MANIFEST_VERSION,
};
pub use profile::{
    measure_index_rebuild, measure_search_latency, measure_startup, measure_workspace_scan,
    measure_xml_parse, run_core_profile, sample_process_memory, CoreProfileSpec, MemorySample,
    ProfileError, ProfileResult,
};
pub use results::{BenchmarkReport, Metric, BENCH_SCHEMA_VERSION};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_reexports_are_usable() {
        // Compile-time smoke test: the public surface of the benchmark module
        // is reachable through the crate root re-exports.
        let cfg = FixtureConfig::for_shape(DatasetShape::Scale, 10, 42);
        let xml = generate_xml(&cfg);
        assert!(xml.starts_with("<?xml"));
        assert_eq!(BENCH_SCHEMA_VERSION, 1);
        assert_eq!(MANIFEST_VERSION, 1);
        assert!(!canonical_datasets(None).is_empty());
        assert!(parse_scale_filter(None) == ScaleFilter::Small);
        let report = BenchmarkReport::with_timestamp("t", "2024-01-01T00:00:00.000Z");
        assert!(report.render_markdown().contains("Benchmark Report"));
        let spec = CoreProfileSpec {
            dataset_name: "smoke",
            workspace_dir: std::path::Path::new("."),
            xml_files: &[],
            parse_iterations: 1,
            search_iterations: 1,
        };
        assert_eq!(spec.dataset_name, "smoke");
    }
}
