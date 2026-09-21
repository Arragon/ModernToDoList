# Profiling Method (PROFILING_METHOD)

Status: Normative for M10 (INH-1113 / RD-M10-011..015, INH-1114 / RD-M10-016..021).
Companion document: `docs/performance/BENCHMARK_FORMAT.md` (result schema).

All backend measurements use `std::time::Instant` and drive the real
production code paths — no synthetic work is ever measured. Implementation:
`src-tauri/src/benchmark/profile.rs`.

## 1. Running a benchmark session

```bash
# from the repository root (Git Bash / PowerShell / cmd)
node scripts/bench/run-benchmark.cjs --scale small        # default: scale-1k only
node scripts/bench/run-benchmark.cjs --scale all --release --out Data/bench/2024-06-01
```

Environment variables:

| Variable          | Values                                          | Default |
| ----------------- | ----------------------------------------------- | ------- |
| `MTL_BENCH_SCALE` | `small` \| `shapes` \| `medium` \| `large` \| `all` | `small` (1k only) |
| `MTL_BENCH_OUT`   | output directory for datasets + reports         | (script chooses) |
| `MTL_BENCH_SEED`  | u64 overriding every dataset seed               | canonical seeds |

The runner invokes the ignored integration test `run_full_benchmark` in
`src-tauri/tests/m10_benchmark_tests.rs`, which:

1. generates the active datasets with the deterministic fixture generator
   (SHA-256 manifest written next to the data),
2. profiles each dataset with `benchmark::profile::run_core_profile`,
3. writes `<dataset>.json` + `<dataset>.md` reports (schema v1).

Release-mode runs (`--release`) are the reportable numbers; debug-mode runs
are for correctness only.

## 2. Backend core metrics (RD-M10-011..015)

| Metric group      | Real code path exercised                                                              |
| ----------------- | ------------------------------------------------------------------------------------- |
| Startup cold      | `infrastructure::db::DatabaseManager::open` on an empty `Data/` (SQLite file creation + `migration::run_migrations`) + `domain::workspace::Workspace::create` + first `scan_documents` — the same sequence `lib.rs::run()` performs at launch. |
| Startup warm      | Second `DatabaseManager::open` + `Workspace::open` + `scan_documents` over identical on-disk state. |
| Workspace scan    | `Workspace::scan_documents` (recursive walk, extension filter) then `Workspace::detect_changes` (BLAKE3 `FileFingerprint` over every registered document). |
| XML parse         | `domain::xml_parser::parse_xml` over dataset bytes; N iterations per file, median per file; reports tasks/s and MiB/s. |
| Index rebuild     | `migration::run_migrations` + `infrastructure::indexer::rebuild_index` into a file-backed SQLite DB (`task_index`, `task_tags`, `task_participants`, `task_dependencies`). |
| Search latency    | Parameterized `title LIKE ?` query over the populated `task_index` — the same table/pattern used by the `query_tasks` IPC command. Search terms are harvested from real indexed titles. p50/p95/p99 + mean + queries/s. When M9's FTS5 layer lands, add `search.fts.latency` metrics alongside (do not remove the LIKE baseline). |

Measurement rules:

- Each measurement gets a fresh scratch directory (`.bench-scratch/`, hidden
  from workspace scans) so cold/warm states are well defined.
- File I/O for parse measurements happens outside the timed region.
- Medians (not means) are reported for parse totals; percentiles for search.
- Results are black-boxed (`std::hint::black_box`) so the optimizer cannot
  elide measured work in release builds.

## 3. Backend memory sampling (RD-M10-016..021)

Windows process memory is sampled with hand-written `extern "system"` FFI —
no extra crate:

```rust
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
}
```

(`K32GetProcessMemoryInfo` is the kernel32.dll forwarder of Psapi's
`GetProcessMemoryInfo`, available since Windows 7; kernel32 is linked by
default, so no build changes are needed.)

Sampled fields → benchmark metrics:

| WinAPI field           | Metric                            | Meaning                          |
| ---------------------- | --------------------------------- | -------------------------------- |
| `WorkingSetSize`       | `memory.process.working_set`      | Current physical footprint.      |
| `PeakWorkingSetSize`   | `memory.process.peak_working_set` | Peak since process start.        |
| `PagefileUsage`        | `memory.process.private_usage`    | Private commit charge.           |

`sample_process_memory()` returns `None` on non-Windows platforms (metrics
are then omitted). During a profiling run the harness samples at report
assembly; peak values therefore cover the whole session because the OS
tracks the high-water mark.

## 4. Frontend Performance API mark contract (RD-M10-016..021)

The frontend (Vue 3 / WebView2) instruments UI performance with the standard
Performance API. This section is the binding contract; the frontend
implementation is tracked separately (this document does not modify any
`src/` file).

### 4.1 Naming convention

- Marks: `mtl:<area>:<event>` (lowercase, colon-separated).
- Measures: `mtl.<area>.<metric>` (dot-separated).
- Every measure is `performance.measure(name, startMark, endMark)`.
- Detail payloads (row counts, byte sizes) go in `measure.detail` via
  `performance.measure(name, { start, end, detail })`.

### 4.2 Required instrumentation points

| Concern                    | Start mark                     | End mark                         | Measure name                | Reported unit |
| -------------------------- | ------------------------------ | -------------------------------- | --------------------------- | ------------- |
| Tree render FPS            | `mtl:tree:render:start`        | `mtl:tree:render:end`            | `mtl.tree.render`           | ms + `fps` (rows / seconds) |
| Inspector edit latency     | `mtl:inspector:edit:start` (input event) | `mtl:inspector:edit:committed` (IPC ack) | `mtl.inspector.edit_latency` | ms |
| Autosave overhead          | `mtl:autosave:start`           | `mtl:autosave:end` (save IPC resolved) | `mtl.autosave.duration` | ms |
| WebView2 cache growth      | n/a (periodic sampling)        | n/a                              | `mtl.webview.cache_estimate` | bytes |

Details:

1. **Tree render FPS** — mark `start` in the TaskTree component's
   `onBeforeUpdate` (or immediately before the virtual/plain list patch) and
   `end` in `onUpdated` of the same tick. Emit `detail: { rows }`. FPS is
   derived as `rows / (duration_ms / 1000)` for full-tree renders, and as
   `1000 / frame_ms` when measuring rAF frame gaps during scrolling:
   sample `requestAnimationFrame` deltas for 2 s while programmatically
   scrolling, report p50/p95 frame time. Decision gate RD-M10-022: if p95
   frame time > 16 ms at 10k+ visible rows → virtualization IMPLEMENT.
2. **Inspector edit latency** — `start` on the first `input` event of an
   editor field; `end` when the corresponding `update_task_field` IPC
   promise resolves. Coalesced (debounced) edits measure from first
   keystroke to final commit.
3. **Autosave overhead** — wrap the session autosave IPC
   (`save_document_atomic`) in marks. Additionally record
   `mtl.autosave.payload_bytes` (serialized size) as a `count`-unit metric.
4. **WebView2 cache growth** — every 60 s sample
   `navigator.storage.estimate()` (`usage` field) and, when the app is
   cross-origin-isolated, `performance.measureUserAgentSpecificMemory()`.
   Report delta-from-baseline as `mtl.webview.cache_estimate` (bytes). The
   on-disk `Data/webview2/` directory size is the backend cross-check.

### 4.3 Export to the benchmark format

A collector (PerformanceObserver over `measure` entries) buffers measures
and flushes them to the backend at session end. Each measure maps 1:1 to a
`Metric` in the schema of `BENCHMARK_FORMAT.md`:

```json
{ "name": "ui.tree.render",          "value": 8.3,  "unit": "ms",   "percentile": 95.0 }
{ "name": "ui.tree.fps",             "value": 58.0, "unit": "fps",  "percentile": null }
{ "name": "ui.inspector.edit",       "value": 21.4, "unit": "ms",   "percentile": 95.0 }
{ "name": "ui.autosave.duration",    "value": 130,  "unit": "ms",   "percentile": null }
{ "name": "ui.webview.cache_growth", "value": 4194304, "unit": "bytes", "percentile": null }
```

UI metrics are stored in the same per-dataset report (`dataset` field set to
`ui-<scenario>`), so JSON tooling does not need a second schema.

## 5. Reproducibility and fairness rules

- Datasets are always the deterministic generated fixtures (fixed canonical
  seeds; see `benchmark::datasets`). Never profile ad-hoc user data.
- Close other applications; disable antivirus on-access scanning of the
  workspace directory or record it in the report notes.
- Run at least 3 repetitions for reportable numbers; the runner writes one
  report per invocation — keep the output directories and compare.
- Record machine identity (CPU, RAM, disk type: HDD/SSD/USB) alongside the
  reports; portable-app numbers on removable drives are a separate category.
