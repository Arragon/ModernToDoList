//! Core profiling harness (RD-M10-011..015, INH-1113) and process memory
//! sampling (RD-M10-016..021, INH-1114).
//!
//! Every measurement drives the REAL production code paths — no synthetic
//! work:
//!
//! - Startup (cold/warm): [`crate::infrastructure::db::DatabaseManager::open`]
//!   (schema creation + migrations on cold, re-open on warm) plus
//!   [`crate::domain::workspace::Workspace`] create/open and an initial
//!   document scan — the same sequence `lib.rs::run()` performs at launch.
//! - Workspace scan: `Workspace::scan_documents` + `Workspace::detect_changes`
//!   (which BLAKE3-hashes every registered document).
//! - XML parse throughput: `domain::xml_parser::parse_xml` over generated
//!   dataset bytes.
//! - Index rebuild: `infrastructure::migration::run_migrations` +
//!   `infrastructure::indexer::rebuild_index` into a real SQLite database.
//! - Search latency: parameterized `title LIKE` queries against the populated
//!   `task_index` table — the same table and pattern used by the
//!   `query_tasks` IPC command (M9 will layer FTS5 on top).
//! - Memory: Windows `K32GetProcessMemoryInfo` via hand-written `extern
//!   "system"` FFI (no extra crate): current + peak working set and private
//!   commit usage.
//!
//! All timings use `std::time::Instant`. Results are emitted as
//! [`Metric`] values consumable by [`BenchmarkReport`].

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use thiserror::Error;

use crate::domain::workspace::{DocumentType, Workspace};
use crate::domain::xml_parser::parse_xml;
use crate::infrastructure::db::DatabaseManager;
use crate::infrastructure::indexer;
use crate::infrastructure::migration;

use super::fixture_gen::count_task_elements;
use super::results::{BenchmarkReport, Metric};

/// Errors produced by the profiling harness.
#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("Database error: {0}")]
    Db(String),
    #[error("Index error: {0}")]
    Index(String),
    #[error("Workspace error: {0}")]
    Workspace(String),
}

pub type ProfileResult<T> = Result<T, ProfileError>;

// ─── Memory sampling (WinAPI FFI, no extra crate) ─────────────────────

/// One point-in-time sample of the current process's memory usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySample {
    /// Current working set size in bytes.
    pub working_set_bytes: u64,
    /// Peak working set size in bytes since process start.
    pub peak_working_set_bytes: u64,
    /// Current private commit (pagefile) usage in bytes.
    pub private_usage_bytes: u64,
}

/// `PROCESS_MEMORY_COUNTERS` from `psapi.h` (SIZE_T = usize on all
/// supported Windows targets).
#[cfg(windows)]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[cfg(windows)]
extern "system" {
    /// Returns the pseudo-handle for the calling process (no close needed).
    fn GetCurrentProcess() -> isize;
    /// kernel32.dll forwarder for Psapi's GetProcessMemoryInfo (Win7+).
    fn K32GetProcessMemoryInfo(
        process: isize,
        counters: *mut ProcessMemoryCounters,
        cb: u32,
    ) -> i32;
}

/// Samples the current process's memory usage.
///
/// Returns `None` on non-Windows platforms or if the WinAPI call fails.
pub fn sample_process_memory() -> Option<MemorySample> {
    #[cfg(windows)]
    {
        unsafe {
            let mut counters = ProcessMemoryCounters::default();
            counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
            let ok = K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb);
            if ok == 0 {
                return None;
            }
            Some(MemorySample {
                working_set_bytes: counters.working_set_size as u64,
                peak_working_set_bytes: counters.peak_working_set_size as u64,
                private_usage_bytes: counters.pagefile_usage as u64,
            })
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Converts a memory sample into benchmark metrics (empty on non-Windows).
pub fn memory_metrics() -> Vec<Metric> {
    match sample_process_memory() {
        Some(s) => vec![
            Metric::new(
                "memory.process.working_set",
                s.working_set_bytes as f64,
                "bytes",
            ),
            Metric::new(
                "memory.process.peak_working_set",
                s.peak_working_set_bytes as f64,
                "bytes",
            ),
            Metric::new(
                "memory.process.private_usage",
                s.private_usage_bytes as f64,
                "bytes",
            ),
        ],
        None => Vec::new(),
    }
}

// ─── Statistics helpers ───────────────────────────────────────────────

/// Nearest-rank percentile over a sorted slice of samples.
///
/// `p` is in 0..=100. Returns 0.0 for empty input.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let clamped = p.clamp(0.0, 100.0);
    let rank = ((clamped / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.max(1).min(sorted.len()) - 1]
}

fn median(sorted: &[Duration]) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[sorted.len() / 2]
}

fn fresh_dir(path: &Path) -> ProfileResult<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

// ─── RD-M10-011: startup (cold/warm) ─────────────────────────────────

/// Measures application startup work against a scratch base directory.
///
/// Cold = fresh `Data/` directory: SQLite file creation + full schema
/// migrations + workspace creation + first scan.
/// Warm = same directories re-opened: existing schema, warm SQLite cache.
pub fn measure_startup(base_dir: &Path) -> ProfileResult<Vec<Metric>> {
    fresh_dir(base_dir)?;
    let data_dir = base_dir.join("data");
    let ws_dir = base_dir.join("workspace");
    fs::create_dir_all(&data_dir)?;
    fs::create_dir_all(&ws_dir)?;

    // Cold start.
    let cold = Instant::now();
    let db_cold = DatabaseManager::open(&data_dir);
    let ws_cold = Workspace::create(&ws_dir, "Benchmark Startup".to_string())
        .map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let cold_docs = ws_cold
        .scan_documents()
        .map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let cold_elapsed = cold.elapsed();
    if !db_cold.is_available() {
        return Err(ProfileError::Db(
            "cold startup: index database unavailable".into(),
        ));
    }

    // Warm start (same on-disk state, second open).
    let warm = Instant::now();
    let db_warm = DatabaseManager::open(&data_dir);
    let ws_warm = Workspace::open(&ws_dir).map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let warm_docs = ws_warm
        .scan_documents()
        .map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let warm_elapsed = warm.elapsed();
    if !db_warm.is_available() {
        return Err(ProfileError::Db(
            "warm startup: index database unavailable".into(),
        ));
    }

    std::hint::black_box((&cold_docs, &warm_docs, &db_cold, &db_warm));
    Ok(vec![
        Metric::new("startup.cold", ms(cold_elapsed), "ms"),
        Metric::new("startup.warm", ms(warm_elapsed), "ms"),
    ])
}

// ─── RD-M10-012: workspace scan ──────────────────────────────────────

/// Measures `scan_documents` (directory walk) and `detect_changes`
/// (BLAKE3 fingerprinting of every registered document) on a workspace root.
///
/// The workspace is created if `workspace.json` does not exist yet.
/// Discovered documents are registered (untimed) before the fingerprinting
/// measurement so `detect_changes` performs real hashing work.
pub fn measure_workspace_scan(workspace_dir: &Path) -> ProfileResult<Vec<Metric>> {
    let mut ws = if workspace_dir.join("workspace.json").exists() {
        Workspace::open(workspace_dir).map_err(|e| ProfileError::Workspace(e.to_string()))?
    } else {
        Workspace::create(workspace_dir, "Benchmark Workspace".to_string())
            .map_err(|e| ProfileError::Workspace(e.to_string()))?
    };

    let t = Instant::now();
    let found = ws
        .scan_documents()
        .map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let scan_elapsed = t.elapsed();

    // Register discovered documents so detect_changes has real files to hash.
    let registered: std::collections::HashSet<String> = ws
        .documents()
        .iter()
        .map(|d| d.file_path.clone())
        .collect();
    for rel in &found {
        let rel_str = rel.to_string_lossy().to_string();
        if !registered.contains(&rel_str) {
            let _ = ws.register_document(rel_str, DocumentType::Managed);
        }
    }
    ws.save().map_err(|e| ProfileError::Workspace(e.to_string()))?;

    let t2 = Instant::now();
    let (_new, _changed, _removed) = ws
        .detect_changes()
        .map_err(|e| ProfileError::Workspace(e.to_string()))?;
    let detect_elapsed = t2.elapsed();

    let scan_secs = scan_elapsed.as_secs_f64().max(1e-9);
    Ok(vec![
        Metric::new("workspace.scan", ms(scan_elapsed), "ms"),
        Metric::new(
            "workspace.scan.files_per_sec",
            found.len() as f64 / scan_secs,
            "files/s",
        ),
        Metric::new("workspace.detect_changes", ms(detect_elapsed), "ms"),
    ])
}

// ─── RD-M10-013: XML parse throughput ────────────────────────────────

/// Measures `parse_xml` over each file (`iterations` runs per file, median
/// per file, summed). Reports total time, task throughput and byte
/// throughput.
pub fn measure_xml_parse(files: &[PathBuf], iterations: usize) -> ProfileResult<Vec<Metric>> {
    let iters = iterations.max(1);
    let mut total = Duration::ZERO;
    let mut total_bytes: u64 = 0;
    let mut total_tasks: usize = 0;

    for path in files {
        let bytes = fs::read(path)?;
        let mut durations = Vec::with_capacity(iters);
        let mut tasks = 0usize;
        for _ in 0..iters {
            let t = Instant::now();
            let doc = parse_xml(&bytes).map_err(|e| {
                ProfileError::Xml(format!("{}: {}", path.display(), e))
            })?;
            durations.push(t.elapsed());
            tasks = count_task_elements(&doc.root);
            std::hint::black_box(&doc);
        }
        durations.sort();
        total += median(&durations);
        total_bytes += bytes.len() as u64;
        total_tasks += tasks;
    }

    let secs = total.as_secs_f64().max(1e-9);
    Ok(vec![
        Metric::new("xml_parse.total", ms(total), "ms"),
        Metric::new(
            "xml_parse.throughput",
            total_tasks as f64 / secs,
            "tasks/s",
        ),
        Metric::new(
            "xml_parse.byte_throughput",
            (total_bytes as f64 / (1024.0 * 1024.0)) / secs,
            "MiB/s",
        ),
        Metric::new("xml_parse.tasks", total_tasks as f64, "count"),
    ])
}

// ─── RD-M10-014: index rebuild ───────────────────────────────────────

/// Creates a real SQLite index database at `db_path`, runs migrations,
/// registers `docs` and measures `indexer::rebuild_index`.
///
/// Returns the metrics plus the populated connection (for search profiling).
pub fn measure_index_rebuild(
    db_path: &Path,
    docs: &[(String, PathBuf)],
) -> ProfileResult<(Vec<Metric>, Connection)> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(db_path);

    let conn = Connection::open(db_path).map_err(|e| ProfileError::Db(e.to_string()))?;
    conn.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;")
        .map_err(|e| ProfileError::Db(e.to_string()))?;
    migration::run_migrations(&conn).map_err(|e| ProfileError::Db(e.to_string()))?;

    let ws_id = "bench-ws";
    conn.execute(
        "INSERT OR REPLACE INTO workspaces (id, name, root_path) VALUES (?1, ?2, ?3)",
        rusqlite::params![ws_id, "Benchmark", "."],
    )
    .map_err(|e| ProfileError::Db(e.to_string()))?;
    for (doc_id, path) in docs {
        conn.execute(
            "INSERT OR REPLACE INTO documents (id, workspace_id, file_path, doc_type) VALUES (?1, ?2, ?3, 'managed')",
            rusqlite::params![doc_id, ws_id, path.to_string_lossy()],
        )
        .map_err(|e| ProfileError::Db(e.to_string()))?;
    }

    let t = Instant::now();
    let task_count = indexer::rebuild_index(&conn, docs, None, None)
        .map_err(|e| ProfileError::Index(e.to_string()))?;
    let elapsed = t.elapsed();

    let secs = elapsed.as_secs_f64().max(1e-9);
    let metrics = vec![
        Metric::new("index_rebuild.total", ms(elapsed), "ms"),
        Metric::new("index_rebuild.tasks", task_count as f64, "count"),
        Metric::new(
            "index_rebuild.throughput",
            task_count as f64 / secs,
            "tasks/s",
        ),
    ];
    Ok((metrics, conn))
}

// ─── RD-M10-015: search latency ──────────────────────────────────────

/// Measures title-substring search latency against the populated
/// `task_index` table (the query path used by the `query_tasks` command).
///
/// Search terms are harvested from real indexed titles. Each term is run
/// `iterations_per_term` times; p50/p95/p99 latencies are reported.
pub fn measure_search_latency(
    conn: &Connection,
    iterations_per_term: usize,
) -> ProfileResult<Vec<Metric>> {
    let terms = harvest_search_terms(conn, 20)?;
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let iters = iterations_per_term.max(1);
    let mut samples: Vec<f64> = Vec::with_capacity(terms.len() * iters);
    let mut queries = 0usize;

    for _ in 0..iters {
        for term in &terms {
            let pattern = format!("%{}%", term);
            let t = Instant::now();
            let mut stmt = conn
                .prepare(
                    "SELECT task_key, document_id, title, priority, status, percent_done \
                     FROM task_index WHERE title LIKE ?1 \
                     ORDER BY document_id, task_key LIMIT 100",
                )
                .map_err(|e| ProfileError::Db(e.to_string()))?;
            let rows = stmt
                .query_map([&pattern], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i32>(3)?))
                })
                .map_err(|e| ProfileError::Db(e.to_string()))?;
            let mut n = 0usize;
            for row in rows {
                std::hint::black_box(row.map_err(|e| ProfileError::Db(e.to_string()))?);
                n += 1;
            }
            samples.push(t.elapsed().as_secs_f64() * 1_000_000.0);
            queries += 1;
            std::hint::black_box(n);
        }
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let total_us: f64 = samples.iter().sum();
    Ok(vec![
        Metric::with_percentile("search.latency", percentile(&samples, 50.0), "us", 50.0),
        Metric::with_percentile("search.latency", percentile(&samples, 95.0), "us", 95.0),
        Metric::with_percentile("search.latency", percentile(&samples, 99.0), "us", 99.0),
        Metric::new("search.latency.mean", total_us / samples.len() as f64, "us"),
        Metric::new(
            "search.throughput",
            queries as f64 / (total_us / 1_000_000.0).max(1e-9),
            "queries/s",
        ),
    ])
}

/// Harvests distinct leading-title words from the index to use as realistic
/// search terms.
fn harvest_search_terms(conn: &Connection, max_terms: usize) -> ProfileResult<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT title FROM task_index ORDER BY task_key LIMIT 500")
        .map_err(|e| ProfileError::Db(e.to_string()))?;
    let titles: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| ProfileError::Db(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let mut terms: Vec<String> = Vec::new();
    for title in titles.iter().step_by(25) {
        if let Some(word) = title.split_whitespace().next() {
            let w = word.to_lowercase();
            if !w.is_empty() && !terms.contains(&w) {
                terms.push(w);
            }
            if terms.len() >= max_terms {
                break;
            }
        }
    }
    Ok(terms)
}

// ─── Full core profile run ───────────────────────────────────────────

/// Input for [`run_core_profile`].
pub struct CoreProfileSpec<'a> {
    /// Dataset name recorded in the report.
    pub dataset_name: &'a str,
    /// Workspace root containing the generated XML file(s). A
    /// `workspace.json` is created here if absent.
    pub workspace_dir: &'a Path,
    /// Absolute paths of the dataset XML files.
    pub xml_files: &'a [PathBuf],
    /// Parse iterations per file (median is used).
    pub parse_iterations: usize,
    /// Search iterations per harvested term.
    pub search_iterations: usize,
}

/// Runs the complete core profiling suite over one dataset and assembles a
/// [`BenchmarkReport`] (startup, workspace scan, XML parse, index rebuild,
/// search latency, process memory).
pub fn run_core_profile(spec: &CoreProfileSpec) -> ProfileResult<BenchmarkReport> {
    let mut report = BenchmarkReport::new(spec.dataset_name);

    let scratch = spec.workspace_dir.join(".bench-scratch");
    fresh_dir(&scratch)?;

    // RD-M10-011: startup (cold/warm) on isolated scratch state.
    report.extend(measure_startup(&scratch.join("startup"))?);

    // RD-M10-012: workspace scan over the dataset directory.
    report.extend(measure_workspace_scan(spec.workspace_dir)?);

    // RD-M10-013: XML parse throughput.
    report.extend(measure_xml_parse(spec.xml_files, spec.parse_iterations)?);

    // RD-M10-014: index rebuild into a real SQLite file.
    let docs: Vec<(String, PathBuf)> = spec
        .xml_files
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let id = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("doc-{}", i));
            (id, p.clone())
        })
        .collect();
    let (index_metrics, conn) =
        measure_index_rebuild(&scratch.join("index.db"), &docs)?;
    report.extend(index_metrics);

    // RD-M10-015: search latency against the populated index.
    report.extend(measure_search_latency(&conn, spec.search_iterations)?);

    // RD-M10-016..021: process memory (Windows; empty elsewhere).
    report.extend(memory_metrics());

    Ok(report)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark::fixture_gen::{generate_file, DatasetShape, FixtureConfig};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mtdl_benchprof_{}_{}_{}",
            tag,
            std::process::id(),
            rand::random::<u32>()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_fixture(dir: &Path, name: &str, tasks: usize) -> PathBuf {
        let cfg = FixtureConfig::for_shape(DatasetShape::Scale, tasks, 4711).with_name(name);
        generate_file(&cfg, dir).unwrap();
        dir.join(cfg.filename)
    }

    #[test]
    fn percentile_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&v, 99.0), 99.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
    }

    #[test]
    fn startup_cold_is_slower_than_warm_typically() {
        let dir = temp_dir("startup");
        let metrics = measure_startup(&dir).unwrap();
        assert_eq!(metrics.len(), 2);
        let cold = metrics.iter().find(|m| m.name == "startup.cold").unwrap();
        let warm = metrics.iter().find(|m| m.name == "startup.warm").unwrap();
        assert_eq!(cold.unit, "ms");
        assert!(cold.value > 0.0, "cold startup must take measurable time");
        assert!(warm.value > 0.0, "warm startup must take measurable time");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn workspace_scan_finds_dataset_files() {
        let dir = temp_dir("wsscan");
        write_fixture(&dir, "ws-scan-a", 100);
        write_fixture(&dir, "ws-scan-b", 100);
        let metrics = measure_workspace_scan(&dir).unwrap();
        let scan = metrics.iter().find(|m| m.name == "workspace.scan").unwrap();
        assert!(scan.value >= 0.0);
        // Two generated xml files were registered and hashed by detect_changes.
        assert!(dir.join("workspace.json").exists());
        let ws = Workspace::open(&dir).unwrap();
        assert_eq!(ws.documents().len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xml_parse_throughput_on_real_files() {
        let dir = temp_dir("xmlparse");
        let path = write_fixture(&dir, "parse-me", 300);
        let metrics = measure_xml_parse(&[path], 3).unwrap();
        let total = metrics.iter().find(|m| m.name == "xml_parse.total").unwrap();
        let tasks = metrics.iter().find(|m| m.name == "xml_parse.tasks").unwrap();
        let tps = metrics.iter().find(|m| m.name == "xml_parse.throughput").unwrap();
        assert_eq!(tasks.value, 300.0);
        assert!(total.value > 0.0);
        assert!(tps.value > 0.0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xml_parse_reports_errors() {
        let dir = temp_dir("xmlparse-bad");
        let bad = dir.join("bad.xml");
        fs::write(&bad, b"not xml at all <<<<").unwrap();
        assert!(measure_xml_parse(&[bad], 1).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn index_rebuild_populates_sqlite() {
        let dir = temp_dir("idxrebuild");
        let path = write_fixture(&dir, "rebuild-me", 200);
        let db_path = dir.join("index.db");
        let docs = vec![("doc-a".to_string(), path.clone())];
        let (metrics, conn) = measure_index_rebuild(&db_path, &docs).unwrap();
        let tasks = metrics
            .iter()
            .find(|m| m.name == "index_rebuild.tasks")
            .unwrap();
        assert_eq!(tasks.value, 200.0);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 200);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn search_latency_over_populated_index() {
        let dir = temp_dir("searchlat");
        let path = write_fixture(&dir, "search-me", 400);
        let db_path = dir.join("index.db");
        let docs = vec![("doc-a".to_string(), path.clone())];
        let (_m, conn) = measure_index_rebuild(&db_path, &docs).unwrap();
        let metrics = measure_search_latency(&conn, 3).unwrap();
        assert!(!metrics.is_empty());
        let p95 = metrics
            .iter()
            .find(|m| m.name == "search.latency" && m.percentile == Some(95.0))
            .unwrap();
        assert!(p95.value > 0.0);
        assert_eq!(p95.unit, "us");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn memory_sampling_windows() {
        match sample_process_memory() {
            Some(s) => {
                // A running test process uses at least a few MiB.
                assert!(s.working_set_bytes > 1024 * 1024);
                assert!(s.peak_working_set_bytes >= s.working_set_bytes);
                assert!(s.private_usage_bytes > 0);
                let metrics = memory_metrics();
                assert_eq!(metrics.len(), 3);
            }
            None => {
                // Non-Windows: sampling is unavailable and yields no metrics.
                assert!(cfg!(not(windows)));
                assert!(memory_metrics().is_empty());
            }
        }
    }

    #[test]
    fn run_core_profile_end_to_end() {
        let dir = temp_dir("coreprofile");
        let path = write_fixture(&dir, "core-ds", 300);
        let spec = CoreProfileSpec {
            dataset_name: "core-ds",
            workspace_dir: &dir,
            xml_files: &[path],
            parse_iterations: 2,
            search_iterations: 2,
        };
        let report = run_core_profile(&spec).unwrap();
        assert_eq!(report.dataset, "core-ds");
        for name in [
            "startup.cold",
            "startup.warm",
            "workspace.scan",
            "xml_parse.total",
            "index_rebuild.total",
        ] {
            assert!(
                report.get(name).is_some(),
                "missing metric {} in report",
                name
            );
        }
        if cfg!(windows) {
            assert!(report.get("memory.process.working_set").is_some());
            assert!(report.get("memory.process.peak_working_set").is_some());
        }
        // The report serializes to the documented JSON schema; reload and
        // compare structurally (float values with a tolerance: JSON decimal
        // round-tripping may differ by 1 ULP).
        let json = report.to_json();
        let parsed = BenchmarkReport::from_json(&json).unwrap();
        assert_eq!(parsed.version, report.version);
        assert_eq!(parsed.timestamp, report.timestamp);
        assert_eq!(parsed.dataset, report.dataset);
        assert_eq!(parsed.metrics.len(), report.metrics.len());
        for (a, b) in parsed.metrics.iter().zip(report.metrics.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.unit, b.unit);
            assert_eq!(a.percentile, b.percentile);
            assert!(
                (a.value - b.value).abs() <= b.value.abs() * 1e-9 + 1e-9,
                "value mismatch for {}: {} vs {}",
                a.name,
                a.value,
                b.value
            );
        }
        fs::remove_dir_all(&dir).ok();
    }
}
