//! Versioned benchmark result format (RD-M10-010, INH-1112).
//!
//! Canonical JSON schema (see `docs/performance/BENCHMARK_FORMAT.md`):
//!
//! ```json
//! {
//!   "version": 1,
//!   "timestamp": "2024-01-01T00:00:00.000Z",
//!   "dataset": "scale-1k",
//!   "metrics": [
//!     { "name": "startup.cold", "value": 12.5, "unit": "ms", "percentile": null },
//!     { "name": "search.latency", "value": 310.0, "unit": "us", "percentile": 95.0 }
//!   ]
//! }
//! ```
//!
//! The same [`BenchmarkReport`] struct renders to a human-readable Markdown
//! report via [`BenchmarkReport::render_markdown`].

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Current benchmark result schema version. Bump on any breaking change to
/// the JSON layout (renamed/removed keys, changed value semantics).
pub const BENCH_SCHEMA_VERSION: u32 = 1;

/// A single measured value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// Dot-separated metric name, e.g. `xml_parse.throughput`.
    pub name: String,
    /// Numeric value (always f64 for schema stability).
    pub value: f64,
    /// Unit string: `ms`, `us`, `s`, `bytes`, `tasks/s`, `files/s`,
    /// `queries/s`, `MiB/s`, `fps`, `count`.
    pub unit: String,
    /// Percentile this value represents (e.g. 50/95/99), or `None` for
    /// aggregate measurements (totals, means, throughputs).
    pub percentile: Option<f64>,
}

impl Metric {
    /// Aggregate (non-percentile) metric.
    pub fn new(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
            percentile: None,
        }
    }

    /// Percentile metric (e.g. p95 search latency).
    pub fn with_percentile(
        name: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
        percentile: f64,
    ) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
            percentile: Some(percentile),
        }
    }
}

/// A complete benchmark report for one dataset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Schema version ([`BENCH_SCHEMA_VERSION`]).
    pub version: u32,
    /// RFC 3339 UTC timestamp of report creation.
    pub timestamp: String,
    /// Dataset name (e.g. `scale-1k`).
    pub dataset: String,
    /// Measured metrics, in collection order.
    pub metrics: Vec<Metric>,
}

impl BenchmarkReport {
    /// Creates an empty report stamped with the current UTC time.
    pub fn new(dataset: impl Into<String>) -> Self {
        Self::with_timestamp(
            dataset,
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        )
    }

    /// Creates an empty report with an explicit timestamp (deterministic
    /// output for tests and golden files).
    pub fn with_timestamp(dataset: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            version: BENCH_SCHEMA_VERSION,
            timestamp: timestamp.into(),
            dataset: dataset.into(),
            metrics: Vec::new(),
        }
    }

    /// Appends one metric.
    pub fn push(&mut self, metric: Metric) {
        self.metrics.push(metric);
    }

    /// Appends many metrics.
    pub fn extend<I: IntoIterator<Item = Metric>>(&mut self, metrics: I) {
        self.metrics.extend(metrics);
    }

    /// Looks up the first metric with the given name.
    pub fn get(&self, name: &str) -> Option<&Metric> {
        self.metrics.iter().find(|m| m.name == name)
    }

    /// Serializes to pretty JSON matching the documented schema.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("BenchmarkReport serializes")
    }

    /// Parses a report from JSON, rejecting unknown schema versions.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let report: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if report.version != BENCH_SCHEMA_VERSION {
            return Err(format!(
                "unsupported benchmark schema version {} (expected {})",
                report.version, BENCH_SCHEMA_VERSION
            ));
        }
        Ok(report)
    }

    /// Renders the human-readable Markdown report.
    pub fn render_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# ModernToDoList Benchmark Report\n\n");
        md.push_str("| Property | Value |\n| --- | --- |\n");
        md.push_str(&format!("| Schema version | {} |\n", self.version));
        md.push_str(&format!("| Dataset | `{}` |\n", self.dataset));
        md.push_str(&format!("| Timestamp | {} |\n", self.timestamp));
        md.push_str(&format!("| Metric count | {} |\n\n", self.metrics.len()));
        md.push_str("## Metrics\n\n");
        md.push_str("| Name | Value | Unit | Percentile |\n");
        md.push_str("| --- | ---: | --- | ---: |\n");
        for m in &self.metrics {
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                m.name,
                format_value(m.value),
                m.unit,
                match m.percentile {
                    Some(p) => format!("p{}", format_value(p)),
                    None => "-".to_string(),
                }
            ));
        }
        md.push_str(&format!(
            "\n_Generated by the ModernToDoList benchmark harness (schema v{})._\n",
            BENCH_SCHEMA_VERSION
        ));
        md
    }

    /// Writes `<stem>.json` and `<stem>.md` into `dir`, returning both paths.
    pub fn write_reports(&self, dir: &Path, stem: &str) -> std::io::Result<(PathBuf, PathBuf)> {
        std::fs::create_dir_all(dir)?;
        let json_path = dir.join(format!("{}.json", stem));
        let md_path = dir.join(format!("{}.md", stem));
        std::fs::write(&json_path, self.to_json())?;
        std::fs::write(&md_path, self.render_markdown())?;
        Ok((json_path, md_path))
    }
}

/// Formats a metric value: integers when integral, otherwise 3 decimals.
fn format_value(v: f64) -> String {
    if v.is_finite() && (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else if v.is_finite() {
        format!("{:.3}", v)
    } else {
        format!("{}", v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> BenchmarkReport {
        let mut r = BenchmarkReport::with_timestamp("scale-1k", "2024-01-01T00:00:00.000Z");
        r.push(Metric::new("startup.cold", 12.5, "ms"));
        r.push(Metric::with_percentile("search.latency", 310.25, "us", 95.0));
        r.push(Metric::new("index_rebuild.tasks", 1000.0, "count"));
        r
    }

    #[test]
    fn schema_version_constant() {
        assert_eq!(BENCH_SCHEMA_VERSION, 1);
        assert_eq!(sample_report().version, BENCH_SCHEMA_VERSION);
    }

    #[test]
    fn json_matches_documented_schema_exactly() {
        let json = sample_report().to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["dataset", "metrics", "timestamp", "version"]);
        assert_eq!(obj["version"], 1);
        assert_eq!(obj["timestamp"], "2024-01-01T00:00:00.000Z");
        assert_eq!(obj["dataset"], "scale-1k");

        let metrics = obj["metrics"].as_array().unwrap();
        assert_eq!(metrics.len(), 3);
        let mut mkeys: Vec<&String> = metrics[0].as_object().unwrap().keys().collect();
        mkeys.sort();
        assert_eq!(mkeys, vec!["name", "percentile", "unit", "value"]);
        assert_eq!(metrics[0]["name"], "startup.cold");
        assert_eq!(metrics[0]["value"], 12.5);
        assert_eq!(metrics[0]["unit"], "ms");
        assert!(metrics[0]["percentile"].is_null());
        assert_eq!(metrics[1]["percentile"], 95.0);
    }

    #[test]
    fn json_round_trip() {
        let r = sample_report();
        let parsed = BenchmarkReport::from_json(&r.to_json()).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn from_json_rejects_other_versions() {
        let mut r = sample_report();
        r.version = 99;
        let err = BenchmarkReport::from_json(&r.to_json()).unwrap_err();
        assert!(err.contains("unsupported benchmark schema version"));
    }

    #[test]
    fn from_json_rejects_garbage() {
        assert!(BenchmarkReport::from_json("not json").is_err());
        assert!(BenchmarkReport::from_json("{}").is_err());
    }

    #[test]
    fn markdown_report_renders() {
        let md = sample_report().render_markdown();
        assert!(md.contains("# ModernToDoList Benchmark Report"));
        assert!(md.contains("| Schema version | 1 |"));
        assert!(md.contains("| Dataset | `scale-1k` |"));
        assert!(md.contains("2024-01-01T00:00:00.000Z"));
        assert!(md.contains("| Name | Value | Unit | Percentile |"));
        assert!(md.contains("| startup.cold | 12.500 | ms | - |"));
        assert!(md.contains("| search.latency | 310.250 | us | p95 |"));
        assert!(md.contains("| index_rebuild.tasks | 1000 | count | - |"));
    }

    #[test]
    fn get_metric_by_name() {
        let r = sample_report();
        assert_eq!(r.get("startup.cold").unwrap().value, 12.5);
        assert!(r.get("missing").is_none());
    }

    #[test]
    fn write_reports_creates_both_files() {
        let dir = std::env::temp_dir().join(format!("mtdl_benchres_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (json_path, md_path) = sample_report().write_reports(&dir, "scale-1k").unwrap();
        assert!(json_path.exists() && md_path.exists());
        let parsed =
            BenchmarkReport::from_json(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert_eq!(parsed, sample_report());
        assert!(std::fs::read_to_string(&md_path)
            .unwrap()
            .contains("Benchmark Report"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn value_formatting() {
        assert_eq!(format_value(1000.0), "1000");
        assert_eq!(format_value(12.5), "12.500");
        assert_eq!(format_value(-3.0), "-3");
    }
}
