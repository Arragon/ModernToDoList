//! Deterministic benchmark fixture generator (RD-M10-001, INH-1110).
//!
//! Generates TDL-dialect XML task trees from an explicitly seeded PRNG
//! (`rand::rngs::SmallRng`). The same seed plus the same [`FixtureConfig`]
//! always yields byte-identical XML — no wall-clock timestamps, no UUIDs and
//! no thread-dependent iteration order are used anywhere in generation.
//!
//! The generated dialect matches the canonical fixtures under
//! `tests/fixtures/xml/canonical/`:
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?><TODOLIST PROJECTNAME="..." NEXTUNIQUEID="..." ...>
//! <TASK ID="1" TITLE="..." POS="0" POSSTRING="1">
//!   <COMMENTS>...</COMMENTS>
//!   <CATEGORY>work</CATEGORY>
//!   <TASK ID="2" ... />
//! </TASK>
//! </TODOLIST>
//! ```
//!
//! Layout rules that guarantee a byte-stable round-trip through the real
//! `domain::xml_parser` / `domain::xml_serializer` pipeline:
//!
//! - No whitespace between the XML declaration and the root element (the
//!   serializer concatenates declaration + root directly).
//! - No trailing newline after `</TODOLIST>`.
//! - Leaf tasks are written self-closing with `/>` (no space), exactly like
//!   `xml_serializer::write_element`.
//! - All attribute values and text content are escaped with
//!   `domain::xml_parser::escape_xml`, which is the inverse of the parser's
//!   unescaping, so parse → serialize is the identity on generated files.
//!
//! A SHA-256 manifest ([`FixtureManifest`]) can be emitted alongside the
//! generated files to certify reproducibility.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use crate::domain::xml_parser::escape_xml;
use crate::domain::xml_tree::{XmlElement, XmlNode};

/// Name recorded in generated manifests.
pub const GENERATOR_NAME: &str = "moderntodolist-benchmark-fixture-gen/1";

/// Version of the manifest file format.
pub const MANIFEST_VERSION: u32 = 1;

/// OLE automation date of the fixed generator epoch (2023-01-01).
const OLE_EPOCH: f64 = 44927.0;

/// Forced-spine depth for the deep-tree shape (guarantees depth >= 20).
const DEEP_SPINE_DEPTH: usize = 22;

/// Vocabulary used for titles/comments. Kept ASCII and free of XML-special
/// characters so escaping is a no-op for titles.
const WORDS: &[&str] = &[
    "analysis", "api", "archive", "audit", "backend", "backup", "benchmark", "branch",
    "browser", "build", "cache", "calendar", "checkpoint", "cleanup", "client", "compile",
    "config", "dashboard", "database", "deadline", "deploy", "design", "document", "editor",
    "email", "export", "feature", "feedback", "filter", "frontend", "gateway", "hotfix",
    "import", "index", "inspector", "integration", "interface", "kernel", "launch", "release",
];

const NAMES: &[&str] = &[
    "A Chen", "B Okafor", "C Silva", "D Ivanov", "E Nakamura", "F Haddad",
    "G Meyer", "H Kim", "I Novak", "J Costa", "K Osei", "L Berg",
];

const TAGS: &[&str] = &[
    "work", "urgent", "office", "home", "review", "design", "qa", "release", "research", "admin",
];

const FILE_EXTS: &[&str] = &["pdf", "docx", "xlsx", "png", "zip"];

/// The structural "shape" of a generated dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatasetShape {
    /// Plain scale dataset: balanced breadth/depth, medium field density.
    Scale,
    /// Deep tree: forced spine of at least [`DEEP_SPINE_DEPTH`] nesting levels.
    DeepTree,
    /// Rich-text heavy: every task carries long escaped-HTML comments.
    RichText,
    /// Attachment heavy: most tasks carry multiple `<FILEREFPATH>` links.
    AttachmentHeavy,
    /// Dependency heavy: most tasks carry multiple `<DEPENDENCY>` elements.
    DependencyHeavy,
}

impl DatasetShape {
    /// Stable kebab-case identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            DatasetShape::Scale => "scale",
            DatasetShape::DeepTree => "deep-tree",
            DatasetShape::RichText => "rich-text",
            DatasetShape::AttachmentHeavy => "attachment-heavy",
            DatasetShape::DependencyHeavy => "dependency-heavy",
        }
    }
}

/// Full configuration of one generated fixture file.
///
/// Every field participates in determinism: identical configs (including the
/// file name, which is embedded in the `FILENAME` root attribute) produce
/// byte-identical XML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureConfig {
    /// Explicit PRNG seed.
    pub seed: u64,
    /// Exact number of `<TASK>` elements to emit.
    pub task_count: usize,
    /// Maximum nesting depth (root tasks are depth 0).
    pub max_depth: usize,
    /// Maximum number of direct children per task.
    pub breadth: usize,
    /// Probability (0.0..=1.0) that an optional field/element is present.
    pub field_density: f64,
    /// Structural shape preset.
    pub shape: DatasetShape,
    /// `PROJECTNAME` root attribute.
    pub project_name: String,
    /// `FILENAME` root attribute and on-disk file name.
    pub filename: String,
}

impl FixtureConfig {
    /// Creates a config with sensible defaults for a given shape.
    pub fn for_shape(shape: DatasetShape, task_count: usize, seed: u64) -> Self {
        let mut cfg = Self {
            seed,
            task_count,
            max_depth: 6,
            breadth: 5,
            field_density: 0.5,
            shape,
            project_name: format!("Benchmark {}", shape.as_str()),
            filename: format!("{}.xml", shape.as_str()),
        };
        match shape {
            DatasetShape::Scale => {}
            DatasetShape::DeepTree => {
                cfg.max_depth = DEEP_SPINE_DEPTH + 2;
                cfg.breadth = 2;
                cfg.field_density = 0.4;
            }
            DatasetShape::RichText => {
                cfg.max_depth = 5;
                cfg.breadth = 6;
                cfg.field_density = 0.9;
            }
            DatasetShape::AttachmentHeavy => {
                cfg.max_depth = 5;
                cfg.breadth = 5;
                cfg.field_density = 0.85;
            }
            DatasetShape::DependencyHeavy => {
                cfg.max_depth = 6;
                cfg.breadth = 5;
                cfg.field_density = 0.75;
            }
        }
        cfg
    }

    /// Builder: override the seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Builder: override the exact task count.
    pub fn with_task_count(mut self, task_count: usize) -> Self {
        self.task_count = task_count;
        self
    }

    /// Builder: override maximum nesting depth.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Builder: override maximum children per task.
    pub fn with_breadth(mut self, breadth: usize) -> Self {
        self.breadth = breadth;
        self
    }

    /// Builder: override optional-field density (clamped to 0.0..=1.0).
    pub fn with_field_density(mut self, density: f64) -> Self {
        self.field_density = density.clamp(0.0, 1.0);
        self
    }

    /// Builder: override project name and file name together (dataset name).
    pub fn with_name(mut self, name: &str) -> Self {
        self.project_name = format!("Benchmark {}", name);
        self.filename = format!("{}.xml", name);
        self
    }
}

/// Mutable generation state threaded through the recursion.
struct Ctx<'a> {
    cfg: &'a FixtureConfig,
    rng: SmallRng,
    next_id: u64,
    emitted: usize,
}

/// Generates a complete TDL XML document as a UTF-8 string.
///
/// Deterministic: `generate_xml(cfg)` called twice with an equal config
/// returns byte-identical strings.
pub fn generate_xml(cfg: &FixtureConfig) -> String {
    let mut out = String::with_capacity(cfg.task_count.saturating_mul(384) + 1024);

    // Declaration is concatenated directly to the root element: the serializer
    // does not re-emit inter-node whitespace outside the root, so any newline
    // here would break byte-stable round-tripping.
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>");
    out.push_str("<TODOLIST");
    push_attr(&mut out, "PROJECTNAME", &cfg.project_name);
    push_attr(&mut out, "EARLIESTDUEDATE", &format_ole(OLE_EPOCH));
    push_attr(&mut out, "LASTMOD", &format_ole(OLE_EPOCH + 365.0));
    push_attr(&mut out, "LASTMODSTRING", "2023-12-31 00:00");
    push_attr(&mut out, "FILENAME", &cfg.filename);
    push_attr(&mut out, "NEXTUNIQUEID", &(cfg.task_count + 1).to_string());
    push_attr(&mut out, "FILEVERSION", "43");
    push_attr(&mut out, "APPVER", "9.0.14.0");
    push_attr(&mut out, "FILEFORMAT", "12");

    if cfg.task_count == 0 {
        out.push_str("/>");
        return out;
    }
    out.push('>');

    let mut ctx = Ctx {
        cfg,
        rng: SmallRng::seed_from_u64(cfg.seed),
        next_id: 1,
        emitted: 0,
    };

    let mut root_index: usize = 0;
    while ctx.emitted < cfg.task_count {
        root_index += 1;
        let pos_path = root_index.to_string();
        gen_task(&mut ctx, &mut out, 0, root_index - 1, &pos_path);
    }

    out.push_str("\n</TODOLIST>");
    out
}

/// Generates one fixture file into `dir` and returns its manifest entry.
///
/// The file is written as UTF-8 without BOM and named `cfg.filename`.
pub fn generate_file(cfg: &FixtureConfig, dir: &Path) -> std::io::Result<ManifestEntry> {
    fs::create_dir_all(dir)?;
    let xml = generate_xml(cfg);
    let bytes = xml.as_bytes();
    let path = dir.join(&cfg.filename);
    fs::write(&path, bytes)?;
    Ok(ManifestEntry {
        file: cfg.filename.clone(),
        bytes: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        task_count: cfg.task_count,
    })
}

/// Computes the lowercase hex SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Counts `<TASK>` elements (at any nesting level) in a parsed tree.
pub fn count_task_elements(elem: &XmlElement) -> usize {
    let mut n = if elem.tag == "TASK" { 1 } else { 0 };
    for child in &elem.children {
        if let XmlNode::Element(e) = child {
            n += count_task_elements(e);
        }
    }
    n
}

/// Returns the maximum `<TASK>` nesting depth (a single root task = 1).
pub fn max_task_depth(elem: &XmlElement) -> usize {
    let own = if elem.tag == "TASK" { 1 } else { 0 };
    let mut best_child = 0usize;
    for child in &elem.children {
        if let XmlNode::Element(e) = child {
            best_child = best_child.max(max_task_depth(e));
        }
    }
    own + best_child
}

// ─── Manifest ─────────────────────────────────────────────────────────

/// One file entry in a fixture manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path of the file relative to the manifest's base directory.
    pub file: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file content.
    pub sha256: String,
    /// Number of `<TASK>` elements the file contains.
    pub task_count: usize,
}

/// SHA-256 manifest over a set of generated fixture files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureManifest {
    /// Manifest format version ([`MANIFEST_VERSION`]).
    pub manifest_version: u32,
    /// Generator identity.
    pub generator: String,
    /// Wall-clock generation time (RFC 3339). Not part of file hashes.
    pub generated_at: String,
    /// File entries, in generation order.
    pub files: Vec<ManifestEntry>,
    /// The configs each file was generated from (full reproducibility).
    pub configs: Vec<FixtureConfig>,
}

impl FixtureManifest {
    /// Creates a manifest skeleton with the current timestamp.
    pub fn new() -> Self {
        Self {
            manifest_version: MANIFEST_VERSION,
            generator: GENERATOR_NAME.to_string(),
            generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            files: Vec::new(),
            configs: Vec::new(),
        }
    }

    /// Records one generated file and its config.
    pub fn push(&mut self, entry: ManifestEntry, cfg: FixtureConfig) {
        self.files.push(entry);
        self.configs.push(cfg);
    }

    /// Combined SHA-256 over all per-file hashes (order-sensitive).
    pub fn manifest_sha256(&self) -> String {
        let joined: String = self
            .files
            .iter()
            .map(|f| format!("{}  {}\n", f.sha256, f.file))
            .collect();
        sha256_hex(joined.as_bytes())
    }

    /// Renders the manifest as `sha256sum`-compatible text.
    pub fn to_sha256_text(&self) -> String {
        self.files
            .iter()
            .map(|f| format!("{}  {}\n", f.sha256, f.file))
            .collect()
    }

    /// Serializes the manifest as pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("FixtureManifest serializes")
    }

    /// Writes `MANIFEST.json` and `MANIFEST.sha256` into `dir`.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(dir.join("MANIFEST.json"), self.to_json())?;
        fs::write(dir.join("MANIFEST.sha256"), self.to_sha256_text())?;
        Ok(())
    }

    /// Re-hashes every listed file under `dir` and returns the entries whose
    /// hash or size no longer matches (empty = manifest verified).
    pub fn verify(&self, dir: &Path) -> std::io::Result<Vec<ManifestEntry>> {
        let mut bad = Vec::new();
        for entry in &self.files {
            let path = dir.join(&entry.file);
            let data = fs::read(&path)?;
            if data.len() as u64 != entry.bytes || sha256_hex(&data) != entry.sha256 {
                bad.push(entry.clone());
            }
        }
        Ok(bad)
    }
}

impl Default for FixtureManifest {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Internals ────────────────────────────────────────────────────────

fn push_attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&escape_xml(value));
    out.push('"');
}

fn format_ole(v: f64) -> String {
    format!("{:.8}", v)
}

fn pick<'a, R: Rng>(rng: &mut R, list: &'a [&'a str]) -> &'a str {
    list[rng.gen_range(0..list.len())]
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn make_title<R: Rng>(rng: &mut R, id: u64) -> String {
    format!(
        "{} {} {} {:06}",
        capitalize(pick(rng, WORDS)),
        pick(rng, WORDS),
        pick(rng, WORDS),
        id
    )
}

fn make_sentence<R: Rng>(rng: &mut R) -> String {
    let n = rng.gen_range(6..15);
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(pick(rng, WORDS));
    }
    s.push('.');
    s
}

fn make_plain_comment<R: Rng>(rng: &mut R) -> String {
    let n = rng.gen_range(1..4);
    let mut parts = Vec::with_capacity(n);
    for _ in 0..n {
        parts.push(make_sentence(rng));
    }
    parts.join(" ")
}

/// Builds an HTML comment body (raw HTML; escaped at write time).
fn make_html_comment<R: Rng>(rng: &mut R, id: u64) -> String {
    let paragraphs = rng.gen_range(3..9);
    let mut html = String::new();
    html.push_str(&format!(
        "<p><strong>Notes for task {:06}</strong></p>",
        id
    ));
    for _ in 0..paragraphs {
        html.push_str(&format!(
            "<p>{} <em>{}</em> {} See <a href=\"https://example.local/doc/{:06}\">reference</a>.</p>",
            make_sentence(rng),
            pick(rng, WORDS),
            make_sentence(rng),
            id
        ));
    }
    html.push_str("<ul>");
    let items = rng.gen_range(2..5);
    for _ in 0..items {
        html.push_str(&format!("<li>{}</li>", make_sentence(rng)));
    }
    html.push_str("</ul>");
    html
}

/// Deterministic calendar date from a day offset since the fixed epoch.
fn date_string(day_offset: i64) -> String {
    let base = chrono::NaiveDate::from_ymd_opt(2023, 1, 1).expect("valid epoch date");
    let date = base + chrono::Duration::days(day_offset);
    date.format("%Y-%m-%d").to_string()
}

/// Deterministic date-time string from day offset + seconds-of-day.
fn datetime_string(day_offset: i64, secs_of_day: u32) -> String {
    let base = chrono::NaiveDate::from_ymd_opt(2023, 1, 1).expect("valid epoch date");
    let date = base + chrono::Duration::days(day_offset);
    format!(
        "{} {:02}:{:02}",
        date.format("%Y-%m-%d"),
        (secs_of_day / 3600) % 24,
        (secs_of_day / 60) % 60
    )
}

fn ole_for(day_offset: i64, secs_of_day: u32) -> String {
    format_ole(OLE_EPOCH + day_offset as f64 + secs_of_day as f64 / 86400.0)
}

/// Number of direct children planned for a task.
///
/// `cfg.max_depth` bounds the total number of nesting levels: tasks exist at
/// depth indices `0..max_depth-1`, so `max_task_depth(root) <= max_depth`.
fn decide_child_count(ctx: &mut Ctx, depth: usize, remaining: usize) -> usize {
    let cfg = ctx.cfg;
    if remaining == 0 || depth + 1 >= cfg.max_depth || cfg.breadth == 0 {
        return 0;
    }
    // Deep-tree shape: force a spine so nesting depth >= DEEP_SPINE_DEPTH.
    if cfg.shape == DatasetShape::DeepTree && depth < DEEP_SPINE_DEPTH {
        return 1;
    }
    let mut count = 0usize;
    let mut p = (0.45 + 0.35 * cfg.field_density) * 0.85f64.powi(depth as i32);
    p = p.clamp(0.0, 1.0);
    while count < cfg.breadth && count < remaining && ctx.rng.gen_bool(p) {
        count += 1;
        p = (p * 0.55).clamp(0.0, 1.0);
    }
    count
}

fn gen_task(ctx: &mut Ctx, out: &mut String, depth: usize, sibling_pos: usize, pos_path: &str) {
    let cfg = ctx.cfg;
    let id = ctx.next_id;
    ctx.next_id += 1;
    ctx.emitted += 1;

    let indent = "  ".repeat(depth);
    let inner_indent = "  ".repeat(depth + 1);
    out.push('\n');
    out.push_str(&indent);

    let remaining = cfg.task_count.saturating_sub(ctx.emitted);
    let child_budget = decide_child_count(ctx, depth, remaining);
    let rich = cfg.shape == DatasetShape::RichText;
    let d = cfg.field_density.clamp(0.0, 1.0);

    // ── Decide optional content (fixed RNG draw order) ────────────────
    let risk: u8 = ctx.rng.gen_range(0..=10);
    let priority: u8 = ctx.rng.gen_range(0..=10);
    let percent_done: u8 = [0u8, 0, 10, 25, 40, 50, 60, 75, 90, 100][ctx.rng.gen_range(0..10)];

    let has_risk = ctx.rng.gen_bool(d);
    let has_created_by = ctx.rng.gen_bool(d);
    let start_offset: i64 = ctx.rng.gen_range(0..365);
    let start_secs: u32 = ctx.rng.gen_range(0..86400);
    let has_start = ctx.rng.gen_bool(d);
    let due_delta: i64 = ctx.rng.gen_range(1..90);
    let has_due = ctx.rng.gen_bool(d);
    let creation_offset: i64 = ctx.rng.gen_range(0..365);
    let creation_secs: u32 = ctx.rng.gen_range(0..86400);
    let has_creation = ctx.rng.gen_bool(d);
    let lastmod_offset: i64 = ctx.rng.gen_range(0..365);
    let lastmod_secs: u32 = ctx.rng.gen_range(0..86400);
    let has_lastmod = ctx.rng.gen_bool(d);
    let has_lastmod_by = ctx.rng.gen_bool(d);
    let alloc_count: usize = ctx.rng.gen_range(1..4);
    let has_allocated_to = ctx.rng.gen_bool(d);
    let has_allocated_by = ctx.rng.gen_bool(d);
    let est_hours: u32 = ctx.rng.gen_range(1..40);
    let has_time_estimate = ctx.rng.gen_bool(d);
    let spent_hours: u32 = ctx.rng.gen_range(0..40);
    let has_time_spent = ctx.rng.gen_bool(d);
    let text_color: u32 = ctx.rng.gen_range(0..0x1_000_000);
    let has_colors = ctx.rng.gen_bool(d);
    let priority_color: u32 = ctx.rng.gen_range(0..0x1_000_000);
    let has_priority_color = ctx.rng.gen_bool(d);
    let sub_total: u8 = ctx.rng.gen_range(1..9);
    let sub_done: u8 = ctx.rng.gen_range(0..=sub_total);
    let has_subtask_done = ctx.rng.gen_bool(d);

    // ── Open tag with attributes ──────────────────────────────────────
    out.push_str("<TASK");
    push_attr(out, "ID", &id.to_string());
    push_attr(out, "TITLE", &make_title(&mut ctx.rng, id));
    push_attr(out, "REFID", "0");
    push_attr(out, "COMMENTSTYPE", if rich { "HTML" } else { "PLAIN_TEXT" });
    if has_created_by {
        push_attr(out, "CREATEDBY", pick(&mut ctx.rng, NAMES));
    }
    push_attr(out, "PRIORITY", &priority.to_string());
    if has_risk {
        push_attr(out, "RISK", &risk.to_string());
    }
    push_attr(out, "PERCENTDONE", &percent_done.to_string());
    if has_start {
        push_attr(out, "STARTDATE", &ole_for(start_offset, start_secs));
        push_attr(out, "STARTDATESTRING", &date_string(start_offset));
    }
    if has_due {
        let due_offset = start_offset + due_delta;
        push_attr(out, "DUEDATE", &ole_for(due_offset, start_secs));
        push_attr(out, "DUEDATESTRING", &date_string(due_offset));
    }
    if has_creation {
        push_attr(out, "CREATIONDATE", &ole_for(creation_offset, creation_secs));
        push_attr(
            out,
            "CREATIONDATESTRING",
            &datetime_string(creation_offset, creation_secs),
        );
    }
    if has_lastmod {
        push_attr(out, "LASTMOD", &ole_for(lastmod_offset, lastmod_secs));
        push_attr(
            out,
            "LASTMODSTRING",
            &datetime_string(lastmod_offset, lastmod_secs),
        );
    }
    if has_lastmod_by {
        push_attr(out, "LASTMODBY", pick(&mut ctx.rng, NAMES));
    }
    push_attr(out, "POS", &sibling_pos.to_string());
    push_attr(out, "POSSTRING", pos_path);
    if has_allocated_to {
        let mut names: Vec<String> = Vec::with_capacity(alloc_count);
        for _ in 0..alloc_count {
            let n = pick(&mut ctx.rng, NAMES).to_string();
            if !names.contains(&n) {
                names.push(n);
            }
        }
        push_attr(out, "ALLOCATEDTO", &names.join("; "));
    }
    if has_allocated_by {
        push_attr(out, "ALLOCATEDBY", pick(&mut ctx.rng, NAMES));
    }
    if has_time_estimate {
        push_attr(out, "TIMEESTIMATE", &format_ole(est_hours as f64));
        push_attr(out, "TIMEESTUNITS", "H");
    }
    if has_time_spent {
        push_attr(out, "TIMESPENT", &format_ole(spent_hours as f64));
        push_attr(out, "TIMESPENTUNITS", "H");
    }
    if has_subtask_done {
        push_attr(out, "SUBTASKDONE", &format!("{}/{}", sub_done, sub_total));
    }
    if has_colors {
        push_attr(out, "TEXTCOLOR", &text_color.to_string());
        push_attr(out, "TEXTWEBCOLOR", &format!("#{:06X}", text_color));
    }
    if has_priority_color {
        push_attr(out, "PRIORITYCOLOR", &priority_color.to_string());
        push_attr(out, "PRIORITYWEBCOLOR", &format!("#{:06X}", priority_color));
    }
    let has_metadata = ctx.rng.gen_bool(d * 0.2);

    // ── Child element plan ────────────────────────────────────────────
    let file_link_count: usize = match cfg.shape {
        DatasetShape::AttachmentHeavy => {
            if ctx.rng.gen_bool(0.85) {
                ctx.rng.gen_range(1..5)
            } else {
                0
            }
        }
        _ => {
            if ctx.rng.gen_bool(d * 0.5) {
                ctx.rng.gen_range(1..3)
            } else {
                0
            }
        }
    };
    let has_comments = rich || ctx.rng.gen_bool(d);
    let category_count: usize = if ctx.rng.gen_bool(d) {
        ctx.rng.gen_range(1..4)
    } else {
        0
    };
    let dependency_count: usize = if id > 1 {
        match cfg.shape {
            DatasetShape::DependencyHeavy => {
                if ctx.rng.gen_bool(0.8) {
                    ctx.rng.gen_range(1..4)
                } else {
                    0
                }
            }
            _ => {
                if ctx.rng.gen_bool(d * 0.3) {
                    1
                } else {
                    0
                }
            }
        }
    } else {
        0
    };

    let has_body = child_budget > 0
        || has_comments
        || category_count > 0
        || file_link_count > 0
        || dependency_count > 0
        || has_metadata;

    if !has_body {
        out.push_str("/>");
        return;
    }
    out.push('>');

    // ── Child elements (canonical TDL order) ──────────────────────────
    for k in 0..file_link_count {
        let ext = pick(&mut ctx.rng, FILE_EXTS);
        let word = pick(&mut ctx.rng, WORDS);
        out.push('\n');
        out.push_str(&inner_indent);
        out.push_str("<FILEREFPATH>");
        out.push_str(&escape_xml(&format!(
            ".\\attachments\\{:06}\\file_{}_{}.{}",
            id, k, word, ext
        )));
        out.push_str("</FILEREFPATH>");
    }
    if has_comments {
        let content = if rich {
            make_html_comment(&mut ctx.rng, id)
        } else {
            make_plain_comment(&mut ctx.rng)
        };
        out.push('\n');
        out.push_str(&inner_indent);
        out.push_str("<COMMENTS>");
        out.push_str(&escape_xml(&content));
        out.push_str("</COMMENTS>");
    }
    let mut cats: Vec<&str> = Vec::with_capacity(category_count);
    for _ in 0..category_count {
        let tag = pick(&mut ctx.rng, TAGS);
        if !cats.contains(&tag) {
            cats.push(tag);
        }
    }
    for cat in cats {
        out.push('\n');
        out.push_str(&inner_indent);
        out.push_str("<CATEGORY>");
        out.push_str(cat);
        out.push_str("</CATEGORY>");
    }
    for _ in 0..dependency_count {
        let target: u64 = ctx.rng.gen_range(1..id);
        let dep_type: u8 = ctx.rng.gen_range(0..3);
        out.push('\n');
        out.push_str(&inner_indent);
        out.push_str(&format!(
            "<DEPENDENCY><TASKID>{}</TASKID><DEPENDENCYTYPE>{}</DEPENDENCYTYPE></DEPENDENCY>",
            target, dep_type
        ));
    }
    if has_metadata {
        let v: u32 = ctx.rng.gen_range(0..1_000_000);
        out.push('\n');
        out.push_str(&inner_indent);
        out.push_str(&format!("<METADATA BENCH0001=\"v{}\"/>", v));
    }

    // ── Nested tasks ──────────────────────────────────────────────────
    for k in 0..child_budget {
        if ctx.emitted >= cfg.task_count {
            break;
        }
        let child_path = format!("{}.{}", pos_path, k + 1);
        gen_task(ctx, out, depth + 1, k, &child_path);
    }

    out.push('\n');
    out.push_str(&indent);
    out.push_str("</TASK>");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{parse_xml, serialize_xml};

    fn small_cfg(seed: u64, count: usize) -> FixtureConfig {
        FixtureConfig::for_shape(DatasetShape::Scale, count, seed).with_name("unit-small")
    }

    #[test]
    fn same_seed_and_config_is_byte_identical() {
        let cfg = small_cfg(1234, 250);
        let a = generate_xml(&cfg);
        let b = generate_xml(&cfg.clone());
        assert_eq!(a, b, "same seed + config must yield byte-identical XML");
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn different_seed_differs() {
        let a = generate_xml(&small_cfg(1, 100));
        let b = generate_xml(&small_cfg(2, 100));
        assert_ne!(a, b);
    }

    #[test]
    fn different_config_differs() {
        let a = generate_xml(&small_cfg(7, 100));
        let b = generate_xml(&small_cfg(7, 101));
        assert_ne!(a, b);
    }

    #[test]
    fn emits_exact_task_count() {
        for count in [1usize, 7, 64, 250] {
            let cfg = small_cfg(99, count);
            let xml = generate_xml(&cfg);
            let doc = parse_xml(xml.as_bytes()).expect("generated XML must parse");
            assert_eq!(
                count_task_elements(&doc.root),
                count,
                "task count mismatch for {count}"
            );
        }
    }

    #[test]
    fn zero_tasks_yields_self_closing_root() {
        let cfg = small_cfg(5, 0);
        let xml = generate_xml(&cfg);
        assert!(xml.ends_with("/>"));
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert_eq!(doc.root.tag, "TODOLIST");
        assert_eq!(count_task_elements(&doc.root), 0);
    }

    #[test]
    fn round_trip_is_byte_stable() {
        for shape in [
            DatasetShape::Scale,
            DatasetShape::DeepTree,
            DatasetShape::RichText,
            DatasetShape::AttachmentHeavy,
            DatasetShape::DependencyHeavy,
        ] {
            let cfg = FixtureConfig::for_shape(shape, 120, 4242).with_name("roundtrip");
            let xml = generate_xml(&cfg);
            let doc = parse_xml(xml.as_bytes()).expect("parse generated");
            let reserialized = serialize_xml(&doc);
            assert_eq!(
                reserialized.as_slice(),
                xml.as_bytes(),
                "byte-stable round-trip failed for shape {}",
                shape.as_str()
            );
        }
    }

    #[test]
    fn deep_tree_shape_reaches_depth_20() {
        let cfg = FixtureConfig::for_shape(DatasetShape::DeepTree, 1000, 77).with_name("deep");
        let xml = generate_xml(&cfg);
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert!(
            max_task_depth(&doc.root) >= 20,
            "deep-tree must nest at least 20 levels, got {}",
            max_task_depth(&doc.root)
        );
    }

    #[test]
    fn max_depth_is_respected() {
        let cfg = small_cfg(3, 200).with_max_depth(3);
        let xml = generate_xml(&cfg);
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert!(max_task_depth(&doc.root) <= 3);
    }

    #[test]
    fn root_attributes_match_tdl_dialect() {
        let cfg = small_cfg(11, 20);
        let xml = generate_xml(&cfg);
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert_eq!(doc.root.tag, "TODOLIST");
        assert_eq!(doc.root.get_attr("NEXTUNIQUEID"), Some("21"));
        assert_eq!(doc.root.get_attr("FILEVERSION"), Some("43"));
        assert_eq!(doc.root.get_attr("APPVER"), Some("9.0.14.0"));
        assert_eq!(doc.root.get_attr("FILEFORMAT"), Some("12"));
        assert_eq!(doc.root.get_attr("FILENAME"), Some("unit-small.xml"));
        assert!(doc.meta.xml_declaration.is_some());
    }

    #[test]
    fn rich_text_shape_produces_html_comments() {
        let cfg = FixtureConfig::for_shape(DatasetShape::RichText, 50, 8).with_name("rich");
        let xml = generate_xml(&cfg);
        let doc = parse_xml(xml.as_bytes()).unwrap();
        assert!(xml.contains("COMMENTSTYPE=\"HTML\""));
        assert!(xml.contains("&lt;p&gt;"), "HTML must be escaped in text");
        // The comment content parses back to real HTML.
        let first_task = doc.root.first_child_by_tag("TASK").unwrap();
        let comments = first_task.first_child_by_tag("COMMENTS").unwrap();
        assert!(comments.text_content().contains("<p>"));
    }

    #[test]
    fn attachment_heavy_shape_produces_file_links() {
        let cfg =
            FixtureConfig::for_shape(DatasetShape::AttachmentHeavy, 100, 9).with_name("attach");
        let xml = generate_xml(&cfg);
        let links = xml.matches("<FILEREFPATH>").count();
        assert!(links >= 50, "expected many file links, got {links}");
    }

    #[test]
    fn dependency_heavy_shape_produces_dependencies() {
        let cfg =
            FixtureConfig::for_shape(DatasetShape::DependencyHeavy, 100, 10).with_name("deps");
        let xml = generate_xml(&cfg);
        let deps = xml.matches("<DEPENDENCY>").count();
        assert!(deps >= 40, "expected many dependencies, got {deps}");
        let doc = parse_xml(xml.as_bytes()).unwrap();
        // Every DEPENDENCY has TASKID + DEPENDENCYTYPE children like TDL.
        fn walk(e: &XmlElement, checked: &mut i32) {
            if e.tag == "DEPENDENCY" {
                assert!(e.first_child_by_tag("TASKID").is_some());
                assert!(e.first_child_by_tag("DEPENDENCYTYPE").is_some());
                *checked += 1;
            }
            for c in &e.children {
                if let XmlNode::Element(ch) = c {
                    walk(ch, checked);
                }
            }
        }
        let mut checked = 0i32;
        walk(&doc.root, &mut checked);
        assert_eq!(checked as usize, deps, "parsed dependency count mismatch");
        assert!(checked >= 40);
    }

    #[test]
    fn ids_are_unique_and_sequential() {
        let cfg = small_cfg(21, 300);
        let xml = generate_xml(&cfg);
        let mut ids: Vec<u64> = Vec::new();
        let mut rest = xml.as_str();
        while let Some(pos) = rest.find("<TASK ID=\"") {
            rest = &rest[pos + 10..];
            let end = rest.find('"').unwrap();
            ids.push(rest[..end].parse().unwrap());
            rest = &rest[end..];
        }
        assert_eq!(ids.len(), 300);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 300, "IDs must be unique");
        assert_eq!(*ids.first().unwrap(), 1);
        assert_eq!(*ids.last().unwrap(), 300);
    }

    #[test]
    fn sha256_hex_known_vector() {
        // SHA-256("abc") standard test vector.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn manifest_records_hashes_and_verifies() {
        let dir = std::env::temp_dir().join(format!("mtdl_fxman_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cfg_a = small_cfg(31, 40).with_name("a");
        let cfg_b = FixtureConfig::for_shape(DatasetShape::RichText, 30, 32).with_name("b");

        let mut manifest = FixtureManifest::new();
        let ea = generate_file(&cfg_a, &dir).unwrap();
        manifest.push(ea.clone(), cfg_a);
        let eb = generate_file(&cfg_b, &dir).unwrap();
        manifest.push(eb.clone(), cfg_b);
        manifest.write(&dir).unwrap();

        // Recomputed hashes match.
        let data_a = fs::read(dir.join("a.xml")).unwrap();
        assert_eq!(sha256_hex(&data_a), ea.sha256);
        assert_eq!(data_a.len() as u64, ea.bytes);

        // Manifest verifies clean, then detects corruption.
        assert!(manifest.verify(&dir).unwrap().is_empty());
        fs::write(dir.join("a.xml"), b"corrupted").unwrap();
        let bad = manifest.verify(&dir).unwrap();
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].file, "a.xml");

        // Files written by the manifest exist.
        assert!(dir.join("MANIFEST.json").exists());
        assert!(dir.join("MANIFEST.sha256").exists());
        let text = fs::read_to_string(dir.join("MANIFEST.sha256")).unwrap();
        assert!(text.contains(&ea.sha256));
        assert!(text.contains("a.xml"));

        // JSON round-trip.
        let parsed: FixtureManifest =
            serde_json::from_str(&fs::read_to_string(dir.join("MANIFEST.json")).unwrap()).unwrap();
        assert_eq!(parsed.files, manifest.files);
        assert_eq!(parsed.manifest_version, MANIFEST_VERSION);
        assert!(!parsed.manifest_sha256().is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_file_is_deterministic_across_calls() {
        let dir = std::env::temp_dir().join(format!("mtdl_fxdet_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cfg = small_cfg(55, 80);
        let e1 = generate_file(&cfg, &dir.join("one")).unwrap();
        let e2 = generate_file(&cfg, &dir.join("two")).unwrap();
        assert_eq!(e1.sha256, e2.sha256);
        assert_eq!(e1.bytes, e2.bytes);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_serializes_stably() {
        let cfg = small_cfg(66, 50);
        let json = serde_json::to_string(&cfg).unwrap();
        let back: FixtureConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);
        assert!(json.contains("\"shape\":\"scale\""));
    }
}
