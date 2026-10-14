# Benchmark Result Format (BENCHMARK_FORMAT)

Version: **1** (`BENCH_SCHEMA_VERSION` in `src-tauri/src/benchmark/results.rs`)
Status: Normative. Applies to all M10 performance artifacts (INH-1112 / RD-M10-010).

## 1. JSON schema

Every benchmark run produces one JSON report per dataset. The schema is
exactly:

```json
{
  "version": 1,
  "timestamp": "2024-06-01T12:00:00.000Z",
  "dataset": "scale-1k",
  "metrics": [
    { "name": "startup.cold",   "value": 15.25, "unit": "ms", "percentile": null },
    { "name": "search.latency", "value": 900.0, "unit": "us", "percentile": 95.0 }
  ]
}
```

| Field       | Type            | Description                                                        |
| ----------- | --------------- | ------------------------------------------------------------------ |
| `version`   | integer (u32)   | Schema version. Consumers MUST reject unknown versions.             |
| `timestamp` | string          | RFC 3339 UTC, millisecond precision (`2024-06-01T12:00:00.000Z`).   |
| `dataset`   | string          | Dataset name, e.g. `scale-1k`, `shape-deep-tree`.                   |
| `metrics`   | array of Metric | Measured values, in collection order. May be empty, never absent.   |

### Metric object

| Field        | Type           | Description                                                        |
| ------------ | -------------- | ------------------------------------------------------------------ |
| `name`       | string         | Dot-separated metric name (see catalogue below).                    |
| `value`      | number (f64)   | Measured value. Always a JSON number, never a string.               |
| `unit`       | string         | One of the allowed units below.                                     |
| `percentile` | number \| null | `50`, `95`, `99` for percentile metrics; `null` for aggregates.     |

### Allowed units

`ms`, `us`, `s`, `bytes`, `count`, `tasks/s`, `files/s`, `queries/s`, `MiB/s`, `fps`

### Versioning policy

- Bump `BENCH_SCHEMA_VERSION` on any breaking change (renamed/removed keys,
  changed semantics of `value`/`percentile`).
- Adding a new metric name or a new unit is NOT breaking (no bump).
- `BenchmarkReport::from_json` rejects reports whose `version` does not match
  the constant, so old tooling never silently misreads new reports.

## 2. Metric name catalogue (core harness)

Produced by `src-tauri/src/benchmark/profile.rs::run_core_profile`:

| Metric                        | Unit      | Percentile | Meaning                                                   |
| ----------------------------- | --------- | ---------- | --------------------------------------------------------- |
| `startup.cold`                | ms        | –          | DB create + migrations + workspace create + first scan.    |
| `startup.warm`                | ms        | –          | Re-open of the same DB/workspace state.                    |
| `workspace.scan`              | ms        | –          | `Workspace::scan_documents` directory walk.                |
| `workspace.scan.files_per_sec`| files/s   | –          | Scan throughput.                                           |
| `workspace.detect_changes`    | ms        | –          | BLAKE3 fingerprinting of all registered documents.         |
| `xml_parse.total`             | ms        | –          | `parse_xml` over the dataset (median of N iterations).     |
| `xml_parse.throughput`        | tasks/s   | –          | Parsed tasks per second.                                   |
| `xml_parse.byte_throughput`   | MiB/s     | –          | Parsed bytes per second.                                   |
| `xml_parse.tasks`             | count     | –          | Number of `<TASK>` elements parsed.                        |
| `index_rebuild.total`         | ms        | –          | `indexer::rebuild_index` into real SQLite.                 |
| `index_rebuild.tasks`         | count     | –          | Tasks written to `task_index`.                             |
| `index_rebuild.throughput`    | tasks/s   | –          | Index rebuild throughput.                                  |
| `search.latency`              | us        | 50/95/99   | Title-substring query latency on `task_index`.             |
| `search.latency.mean`         | us        | –          | Mean query latency.                                        |
| `search.throughput`           | queries/s | –          | Queries per second.                                        |
| `memory.process.working_set`  | bytes     | –          | WinAPI working set at end of run (Windows only).           |
| `memory.process.peak_working_set` | bytes | –          | Peak working set since process start (Windows only).       |
| `memory.process.private_usage`| bytes     | –          | Private commit / pagefile usage (Windows only).            |

Frontend UI metrics (tree render FPS, inspector edit latency, autosave
overhead, WebView2 cache growth) use the same JSON schema; their Performance
API mark contract is specified in `docs/performance/PROFILING_METHOD.md`.

## 3. Markdown report

`BenchmarkReport::render_markdown` renders the identical struct as:

```markdown
# ModernToDoList Benchmark Report

| Property | Value |
| --- | --- |
| Schema version | 1 |
| Dataset | `scale-1k` |
| Timestamp | 2024-06-01T12:00:00.000Z |
| Metric count | 2 |

## Metrics

| Name | Value | Unit | Percentile |
| --- | ---: | --- | ---: |
| startup.cold | 15.250 | ms | - |
| search.latency | 900 | us | p95 |
```

Value formatting: integral values print without decimals; all others print
with 3 decimals. Percentile `null` renders as `-`; a percentile `p` renders
as `p50` / `p95` / `p99`.

## 4. File layout of a benchmark run

`scripts/bench/run-benchmark.cjs --out <dir>` produces:

```text
<dir>/
  datasets/
    MANIFEST.json          # fixture manifest (files, sizes, SHA-256, configs)
    MANIFEST.sha256        # sha256sum-compatible text manifest
    scale-1k/scale-1k.xml
    ...
  reports/
    scale-1k.json          # BenchmarkReport (schema above)
    scale-1k.md            # Markdown rendering
    ...
```

## 5. Fixture manifest format

`MANIFEST.json` (from `src-tauri/src/benchmark/fixture_gen.rs`):

```json
{
  "manifest_version": 1,
  "generator": "moderntodolist-benchmark-fixture-gen/1",
  "generated_at": "2024-06-01T12:00:00.000Z",
  "files": [
    { "file": "scale-1k/scale-1k.xml", "bytes": 384512, "sha256": "…64 hex…", "task_count": 1000 }
  ],
  "configs": [ { "seed": 5572628853046247425, "task_count": 1000, "...": "..." } ]
}
```

`configs` embeds the full `FixtureConfig` of every file, so any consumer can
regenerate byte-identical fixtures. `MANIFEST.sha256` contains
`<sha256>  <file>` lines (two spaces), compatible with `sha256sum -c`.

## 6. Reproducibility contract

Same `FixtureConfig` (seed + task_count + max_depth + breadth + field_density
+ shape + names) ⇒ byte-identical XML, on any machine, in any run. Generation
never reads the clock, never uses UUIDs, and iterates no hash maps. Enforced
by tests in `src-tauri/src/benchmark/fixture_gen.rs` and
`src-tauri/tests/m10_benchmark_tests.rs`.
