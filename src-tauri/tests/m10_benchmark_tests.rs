//! M10 Benchmark Integration Tests (RD-M10-001 .. RD-M10-021).
//!
//! Covers:
//! - INH-1110: deterministic fixture generation + SHA-256 manifest
//! - INH-1111: canonical dataset catalogue, small-scale path, env gating
//! - INH-1112: versioned benchmark result format (JSON + Markdown)
//! - INH-1113: core profiling harness against real code paths
//! - INH-1114: Windows process memory sampling
//!
//! Large scales (10k/50k/100k) are gated behind `MTL_BENCH_SCALE`; the
//! `run_full_benchmark` test (ignored by default) drives the complete suite
//! and is invoked by `scripts/bench/run-benchmark.cjs`.

use std::fs;
use std::path::PathBuf;

use moderntodolist_lib::benchmark::datasets::{
    self, canonical_datasets, dataset_by_name, filter_allows, generate_dataset, generate_datasets,
    parse_scale_filter, ScaleFilter,
};
use moderntodolist_lib::benchmark::fixture_gen::{
    count_task_elements, generate_xml, max_task_depth, sha256_hex, DatasetShape, FixtureConfig,
    FixtureManifest,
};
use moderntodolist_lib::benchmark::profile::{
    measure_xml_parse, run_core_profile, sample_process_memory, CoreProfileSpec,
};
use moderntodolist_lib::benchmark::results::{BenchmarkReport, Metric, BENCH_SCHEMA_VERSION};
use moderntodolist_lib::domain::{parse_xml, serialize_xml};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mtdl_m10_{}_{}_{}",
        tag,
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ─── INH-1110: deterministic fixture generator ───────────────────────

#[test]
fn m10_001_same_seed_and_config_is_byte_identical() {
    let cfg = FixtureConfig::for_shape(DatasetShape::Scale, 500, 0xC0FFEE).with_name("det-proof");
    let a = generate_xml(&cfg);
    let b = generate_xml(&cfg);
    assert_eq!(a.as_bytes(), b.as_bytes(), "same seed+config => identical bytes");
    assert_eq!(sha256_hex(a.as_bytes()), sha256_hex(b.as_bytes()));

    // A different seed must diverge.
    let c = generate_xml(&cfg.clone().with_seed(0xBEEF));
    assert_ne!(a, c);
}

#[test]
fn m10_001_configurable_depth_breadth_density() {
    let deep = FixtureConfig::for_shape(DatasetShape::Scale, 400, 1)
        .with_max_depth(2)
        .with_breadth(3);
    let doc = parse_xml(generate_xml(&deep).as_bytes()).unwrap();
    assert!(max_task_depth(&doc.root) <= 2);

    let dense = FixtureConfig::for_shape(DatasetShape::Scale, 400, 2).with_field_density(1.0);
    let sparse = FixtureConfig::for_shape(DatasetShape::Scale, 400, 2).with_field_density(0.0);
    // Density 1.0 emits far more optional attributes than density 0.0.
    assert!(generate_xml(&dense).len() > generate_xml(&sparse).len());
}

#[test]
fn m10_001_manifest_lists_files_with_sha256() {
    let dir = temp_dir("manifest");
    let specs: Vec<_> = vec![
        dataset_by_name("scale-1k", None).unwrap(),
        dataset_by_name("shape-rich-text", None).unwrap(),
    ];
    let manifest = generate_datasets(&dir, &specs).unwrap();
    assert_eq!(manifest.files.len(), 2);
    for entry in &manifest.files {
        let data = fs::read(dir.join(&entry.file)).unwrap();
        assert_eq!(sha256_hex(&data), entry.sha256);
        assert_eq!(data.len() as u64, entry.bytes);
        assert!(entry.sha256.len() == 64);
    }
    assert!(manifest.verify(&dir).unwrap().is_empty());
    fs::remove_dir_all(&dir).ok();
}

// ─── INH-1111: canonical datasets ────────────────────────────────────

#[test]
fn m10_002_scale_variants_exist_for_all_sizes() {
    let all = canonical_datasets(None);
    for (name, count) in [
        ("scale-1k", 1_000usize),
        ("scale-10k", 10_000),
        ("scale-50k", 50_000),
        ("scale-100k", 100_000),
    ] {
        let spec = all.iter().find(|s| s.name == name).expect(name);
        assert_eq!(spec.task_count(), count);
    }
}

#[test]
fn m10_003_shape_variants_have_required_properties() {
    let all = canonical_datasets(None);
    let shapes = [
        "shape-deep-tree",
        "shape-rich-text",
        "shape-attachment-heavy",
        "shape-dependency-heavy",
    ];
    for name in shapes {
        assert!(
            all.iter().any(|s| s.name == name),
            "missing shape dataset {name}"
        );
    }
    let deep = all.iter().find(|s| s.name == "shape-deep-tree").unwrap();
    assert!(deep.config.max_depth >= 20);

    // Generated content actually exhibits the shapes.
    let dir = temp_dir("shapes");
    let deep_entry = generate_dataset(&dir, deep).unwrap();
    let bytes = fs::read(dir.join(&deep_entry.file)).unwrap();
    let doc = parse_xml(&bytes).unwrap();
    assert!(max_task_depth(&doc.root) >= 20, "deep-tree must nest >= 20");

    let rich = all.iter().find(|s| s.name == "shape-rich-text").unwrap();
    let rich_entry = generate_dataset(&dir, rich).unwrap();
    let rich_xml = fs::read_to_string(dir.join(&rich_entry.file)).unwrap();
    assert!(rich_xml.contains("COMMENTSTYPE=\"HTML\""));

    let att = all
        .iter()
        .find(|s| s.name == "shape-attachment-heavy")
        .unwrap();
    let att_entry = generate_dataset(&dir, att).unwrap();
    let att_xml = fs::read_to_string(dir.join(&att_entry.file)).unwrap();
    assert!(att_xml.matches("<FILEREFPATH>").count() >= 500);

    let dep = all
        .iter()
        .find(|s| s.name == "shape-dependency-heavy")
        .unwrap();
    let dep_entry = generate_dataset(&dir, dep).unwrap();
    let dep_xml = fs::read_to_string(dir.join(&dep_entry.file)).unwrap();
    assert!(dep_xml.matches("<DEPENDENCY>").count() >= 400);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn m10_004_large_scales_gated_by_env_default_small() {
    // Default (no env): only scale-1k is active.
    assert_eq!(parse_scale_filter(None), ScaleFilter::Small);
    let small = datasets::specs_for_filter(ScaleFilter::Small, None);
    assert_eq!(small.len(), 1);
    assert_eq!(small[0].name, "scale-1k");
    assert!(!filter_allows(ScaleFilter::Small, &dataset_by_name("scale-50k", None).unwrap()));

    // Every scale remains reachable on demand regardless of the gate.
    for name in ["scale-1k", "scale-10k", "scale-50k", "scale-100k"] {
        assert!(dataset_by_name(name, None).is_some(), "{name} on demand");
    }
    assert_eq!(
        datasets::specs_for_filter(ScaleFilter::All, None).len(),
        8,
        "full catalogue available via explicit filter"
    );
}

#[test]
fn m10_005_small_dataset_parses_with_real_parser_and_round_trips() {
    let dir = temp_dir("smallrt");
    let spec = dataset_by_name("scale-1k", None).unwrap();
    let entry = generate_dataset(&dir, &spec).unwrap();
    let path = dir.join(&entry.file);
    let bytes = fs::read(&path).unwrap();

    // Real parser accepts the dataset and sees exactly 1000 tasks.
    let doc = parse_xml(&bytes).expect("scale-1k must parse");
    assert_eq!(doc.root.tag, "TODOLIST");
    assert_eq!(count_task_elements(&doc.root), 1_000);

    // Real parser + real serializer round-trip byte-stably.
    let out = serialize_xml(&doc);
    assert_eq!(out, bytes, "generated dataset must round-trip byte-stably");

    // Domain mapping also reads every task.
    let task = doc.root.first_child_by_tag("TASK").expect("has root task");
    let mapped = moderntodolist_lib::domain::read_task(task);
    assert!(!mapped.title.is_empty());
    assert!(task.get_attr("ID").is_some());

    fs::remove_dir_all(&dir).ok();
}

// ─── INH-1112: versioned benchmark result format ─────────────────────

#[test]
fn m10_010_result_format_json_schema_and_markdown() {
    let mut report = BenchmarkReport::with_timestamp("scale-1k", "2024-06-01T12:00:00.000Z");
    report.push(Metric::new("startup.cold", 15.25, "ms"));
    report.push(Metric::with_percentile("search.latency", 900.0, "us", 95.0));
    assert_eq!(report.version, BENCH_SCHEMA_VERSION);

    let v: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
    let obj = v.as_object().unwrap();
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["dataset", "metrics", "timestamp", "version"]);
    let m0 = &obj["metrics"][0];
    assert_eq!(m0["name"], "startup.cold");
    assert_eq!(m0["unit"], "ms");
    assert!(m0["percentile"].is_null());
    assert_eq!(obj["metrics"][1]["percentile"], 95.0);

    let md = report.render_markdown();
    assert!(md.contains("# ModernToDoList Benchmark Report"));
    assert!(md.contains("| startup.cold | 15.250 | ms | - |"));
    assert!(md.contains("| search.latency | 900 | us | p95 |"));

    // Reports persist as JSON + Markdown side by side.
    let dir = temp_dir("reports");
    let (json_path, md_path) = report.write_reports(&dir, "scale-1k").unwrap();
    assert!(json_path.exists() && md_path.exists());
    let reloaded = BenchmarkReport::from_json(&fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(reloaded, report);
    fs::remove_dir_all(&dir).ok();
}

// ─── INH-1113/1114: profiling harness ────────────────────────────────

#[test]
fn m10_013_core_profile_runs_real_code_paths() {
    let dir = temp_dir("coreprof");
    let spec = dataset_by_name("scale-1k", None).unwrap();
    let entry = generate_dataset(&dir, &spec).unwrap();
    let xml_path = dir.join(&entry.file);

    let profile_spec = CoreProfileSpec {
        dataset_name: &spec.name,
        workspace_dir: &dir,
        xml_files: &[xml_path.clone()],
        parse_iterations: 2,
        search_iterations: 2,
    };
    let report = run_core_profile(&profile_spec).unwrap();
    assert_eq!(report.dataset, "scale-1k");
    assert_eq!(report.version, BENCH_SCHEMA_VERSION);

    for name in [
        "startup.cold",
        "startup.warm",
        "workspace.scan",
        "workspace.detect_changes",
        "xml_parse.total",
        "xml_parse.throughput",
        "index_rebuild.total",
        "index_rebuild.tasks",
        "search.latency",
    ] {
        assert!(report.get(name).is_some(), "report missing metric {name}");
    }
    assert_eq!(report.get("index_rebuild.tasks").unwrap().value, 1_000.0);
    assert_eq!(report.get("xml_parse.tasks").unwrap().value, 1_000.0);

    // Percentile triple present for search latency.
    let percentiles: Vec<f64> = report
        .metrics
        .iter()
        .filter(|m| m.name == "search.latency")
        .filter_map(|m| m.percentile)
        .collect();
    assert!(percentiles.contains(&50.0));
    assert!(percentiles.contains(&95.0));
    assert!(percentiles.contains(&99.0));

    // Scratch dir is hidden from workspace scans and index rebuild worked.
    assert!(dir.join(".bench-scratch").join("index.db").exists());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn m10_013_parse_throughput_measurable() {
    let dir = temp_dir("parsethroughput");
    let cfg = FixtureConfig::for_shape(DatasetShape::Scale, 500, 31337).with_name("tp");
    let entry = moderntodolist_lib::benchmark::generate_file(&cfg, &dir).unwrap();
    let metrics = measure_xml_parse(&[dir.join(&entry.file)], 3).unwrap();
    let total = metrics.iter().find(|m| m.name == "xml_parse.total").unwrap();
    assert!(total.value > 0.0, "parse must take measurable time");
    assert_eq!(
        metrics.iter().find(|m| m.name == "xml_parse.tasks").unwrap().value,
        500.0
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn m10_016_memory_sampling_current_and_peak() {
    match sample_process_memory() {
        Some(s) => {
            assert!(cfg!(windows), "sampling only supported on Windows");
            assert!(s.working_set_bytes > 1024 * 1024, "working set implausibly small");
            assert!(s.peak_working_set_bytes >= s.working_set_bytes, "peak < current");
        }
        None => assert!(!cfg!(windows), "Windows sampling must succeed"),
    }
}

// ─── Full benchmark run (driven by scripts/bench/run-benchmark.cjs) ──

/// Runs the complete benchmark over all datasets selected by
/// `MTL_BENCH_SCALE` (default: scale-1k), writing JSON + Markdown reports
/// into `MTL_BENCH_OUT`. Ignored by default because large scales are slow;
/// invoke via `scripts/bench/run-benchmark.cjs` or:
///
/// ```text
/// MTL_BENCH_OUT=<dir> cargo test --test m10_benchmark_tests -- --ignored --exact run_full_benchmark --nocapture
/// ```
#[test]
#[ignore = "full benchmark run; use scripts/bench/run-benchmark.cjs"]
fn run_full_benchmark() {
    let out = match std::env::var("MTL_BENCH_OUT") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => {
            println!("BENCH_SKIP: set MTL_BENCH_OUT=<dir> to run the full benchmark");
            return;
        }
    };
    let datasets_dir = out.join("datasets");
    let reports_dir = out.join("reports");
    fs::create_dir_all(&reports_dir).unwrap();

    let specs = datasets::active_datasets();
    assert!(!specs.is_empty(), "no active datasets");
    println!("BENCH_DATASETS={}", specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(","));

    let manifest = generate_datasets(&datasets_dir, &specs).unwrap();
    println!("BENCH_MANIFEST={}", datasets_dir.join("MANIFEST.json").display());
    println!("BENCH_MANIFEST_SHA256={}", manifest.manifest_sha256());

    for spec in &specs {
        let workspace_dir = datasets_dir.join(&spec.name);
        let xml_path = workspace_dir.join(&spec.config.filename);
        assert!(xml_path.exists(), "dataset file missing: {:?}", xml_path);

        let profile = CoreProfileSpec {
            dataset_name: &spec.name,
            workspace_dir: &workspace_dir,
            xml_files: &[xml_path],
            parse_iterations: 3,
            search_iterations: 5,
        };
        let report = run_core_profile(&profile).unwrap_or_else(|e| {
            panic!("profiling {} failed: {e}", spec.name)
        });
        let stem = spec.name.replace('/', "_");
        let (json_path, md_path) = report.write_reports(&reports_dir, &stem).unwrap();
        println!("BENCH_REPORT_JSON={}", json_path.display());
        println!("BENCH_REPORT_MD={}", md_path.display());
        println!(
            "BENCH_SUMMARY {} metrics={} xml_parse={}ms index_rebuild={}ms",
            spec.name,
            report.metrics.len(),
            fmt(report.get("xml_parse.total")),
            fmt(report.get("index_rebuild.total")),
        );
    }
    println!("BENCH_DONE");
}

fn fmt(m: Option<&Metric>) -> String {
    m.map(|m| format!("{:.3}", m.value)).unwrap_or_else(|| "n/a".into())
}

// ─── Manifest reproducibility across directories ─────────────────────

#[test]
fn m10_006_manifest_reproducible_across_runs() {
    let dir_a = temp_dir("repro-a");
    let dir_b = temp_dir("repro-b");
    let specs = vec![dataset_by_name("scale-1k", None).unwrap()];
    let ma = generate_datasets(&dir_a, &specs).unwrap();
    let mb = generate_datasets(&dir_b, &specs).unwrap();
    assert_eq!(ma.files, mb.files, "file entries must be identical");
    assert_eq!(ma.manifest_sha256(), mb.manifest_sha256());

    // A corrupting edit is caught by verify().
    let victim = dir_a.join("scale-1k").join("scale-1k.xml");
    let mut data = fs::read(&victim).unwrap();
    data[100] ^= 0xFF;
    fs::write(&victim, &data).unwrap();
    let bad = ma.verify(&dir_a).unwrap();
    assert_eq!(bad.len(), 1);

    fs::remove_dir_all(&dir_a).ok();
    fs::remove_dir_all(&dir_b).ok();
}

/// Sanity: the manifest JSON on disk reloads into the same struct.
#[test]
fn m10_007_manifest_json_roundtrip() {
    let dir = temp_dir("manjson");
    let specs = vec![dataset_by_name("scale-1k", None).unwrap()];
    let m = generate_datasets(&dir, &specs).unwrap();
    let text = fs::read_to_string(dir.join("MANIFEST.json")).unwrap();
    let reloaded: FixtureManifest = serde_json::from_str(&text).unwrap();
    assert_eq!(reloaded.files, m.files);
    assert_eq!(reloaded.configs, m.configs);
    assert_eq!(reloaded.manifest_version, 1);
    fs::remove_dir_all(&dir).ok();
}
