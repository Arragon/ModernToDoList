//! Cross-document transfer transactions (M8): copy / move task subtrees
//! between TDL documents with a TARGET-FIRST commit protocol.
//!
//! # Invariant (P0, GATE-M8)
//!
//! A task must NEVER be lost. An interrupted transfer must leave a
//! DUPLICATE rather than a loss. Concretely:
//!
//! - The target document is committed (atomic replace, validated) BEFORE the
//!   source document is touched in any destructive way.
//! - Every phase transition is recorded durably (fsync) in a
//!   [`super::transfer_journal::TransferJournal`] before the phase it
//!   authorizes begins.
//! - The source subtree is only removed after a verified `TargetCommitted`
//!   journal entry AND a fresh source-fingerprint revalidation (detects
//!   concurrent external edits).
//! - Recovery never deletes data it cannot prove is duplicated: it biases
//!   toward preservation and reports `CompletedWithDuplicate` instead.
//!
//! # Pipeline
//!
//! ```text
//! plan ──► journal(Started) ──► stage assets ──► journal(AssetsStaged)
//!      ──► write target temp ──► validate ──► atomic replace ──► journal(TargetCommitted)
//!      ──► [copy] journal(Completed) ──► cleanup
//!      ──► [move] backup source ──► journal(SourceBackedUp)
//!               ──► revalidate source fingerprint ──► remove subtree
//!               ──► atomic replace source ──► journal(SourceCommitted)
//!               ──► journal(Completed) ──► cleanup
//! ```
//!
//! The atomic-replace steps reuse the M3 machinery
//! ([`super::persistence::atomic_save`]) so temp-file/backup/validation
//! semantics are identical to normal saves.
//!
//! # Losslessness
//!
//! The transferred payload is a deep clone of the source `<TASK>` XML element
//! (the lossless M2 tree), so unknown attributes, unknown child elements,
//! comments, CDATA and raw METADATA survive the transfer byte-for-byte apart
//! from the deliberately rewritten references (IDs, dependencies, asset
//! paths). The domain [`Task`] snapshot is used for planning and validation
//! only.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::fingerprint::FileFingerprint;
use super::id_allocator::TaskIdAllocator;
use super::mappers::read_task;
use super::persistence::{atomic_save, SaveConfig};
use super::session::SaveErrorCode;
use super::task::{CommentType, Task, TaskTree};
use super::transfer_journal::{
    default_journal_dir, scan_journal_dir, JournalError, JournalPhase, ScannedJournal,
    TransferJournal,
};
use super::types::TaskId;
use super::xml_parser::parse_xml;
use super::xml_serializer::serialize_xml;
use super::xml_tree::{XmlDocument, XmlElement, XmlNode};
use super::workspace::Workspace;

// ─────────────────────────── Core types ───────────────────────────

/// The kind of transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferOperation {
    /// Insert a clone into the target; the source is NEVER modified.
    Copy,
    /// Insert a clone into the target, then remove the subtree from the source.
    Move,
}

impl Default for TransferOperation {
    /// Copy is the safe default: it never modifies any source document.
    fn default() -> Self {
        TransferOperation::Copy
    }
}

/// Policy for dependencies whose other endpoint lies OUTSIDE the transferred
/// subtree. See `docs/multidoc/EXTERNAL_DEPENDENCY_POLICY.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalDependencyPolicy {
    /// Default. Keep the `<TASKID>` value unchanged and add a `<FILENAME>`
    /// marker child naming the source document, so the reference degrades to
    /// an External ref instead of silently pointing at an unrelated task in
    /// the target document. A warning is emitted for each occurrence.
    PreserveAsExternal,
    /// Abort the whole transfer with
    /// [`TransferError::ExternalDependenciesBlocked`] before any side effect.
    Block,
}

impl Default for ExternalDependencyPolicy {
    fn default() -> Self {
        ExternalDependencyPolicy::PreserveAsExternal
    }
}

/// One endpoint document of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentEndpoint {
    /// Workspace `DocumentId` when known (stable across renames).
    pub document_id: Option<String>,
    /// Absolute path of the XML document.
    pub path: PathBuf,
    /// BLAKE3 fingerprint of the file bytes at planning time.
    pub fingerprint: FileFingerprint,
}

/// How an asset (file link / embedded reference) participates in a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClassification {
    /// A document-relative file that must be physically copied next to the
    /// target document, with the reference rewritten.
    CopyRequired,
    /// URLs and absolute local paths: the reference is kept verbatim.
    ReferenceOnly,
    /// Missing file or already-identical file at the destination: no copy.
    Skip,
}

/// A planned asset action for one file reference inside the subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetPlanEntry {
    /// Source task (pre-map ID) owning the reference.
    pub owner_task_id: String,
    /// The raw reference text as it appears in the XML (FILEREFPATH / src).
    pub original_ref: String,
    /// Classification decision.
    pub classification: AssetClassification,
    /// Resolved absolute source file (CopyRequired / Skip-missing).
    pub source_abs: Option<PathBuf>,
    /// Planned absolute destination (CopyRequired only).
    pub target_abs: Option<PathBuf>,
    /// Rewritten reference text, when it differs from `original_ref`.
    pub rewritten_ref: Option<String>,
    /// BLAKE3 hex of the source file at planning time (CopyRequired).
    pub hash: Option<String>,
    /// Human-readable reason for ReferenceOnly / Skip decisions.
    pub reason: Option<String>,
}

/// Non-fatal issue surfaced to the user after a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransferWarning {
    /// An external dependency was preserved as an External ref.
    ExternalDependencyPreserved {
        task_id: String,
        dep_task_id: String,
        source_doc: String,
    },
    /// A referenced asset file does not exist; the reference was kept.
    AssetMissing { task_id: String, path: String },
    /// An absolute local path reference was kept verbatim (not copied).
    AssetAbsoluteReferenceKept { task_id: String, path: String },
    /// A URL reference was kept verbatim.
    AssetUrlKept { task_id: String, url: String },
    /// A destination name collision was resolved with a unique file name.
    AssetCollisionRenamed { from: String, to: String },
    /// A task in the subtree uses a non-numeric ID (TDL expects integers).
    NonNumericTaskId { id: String },
    /// After a move, remaining source tasks keep dependencies that now point
    /// at the moved (removed) tasks. They are preserved as unresolved refs.
    OrphanedSourceDependency { task_id: String, dep_task_id: String },
}

/// The immutable contract of one transfer transaction (spec RD-M8-001).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferManifest {
    /// Unique transaction id (UUID v4).
    pub transaction_id: String,
    /// Copy or Move.
    pub operation: TransferOperation,
    /// Source document endpoint (path + fingerprint at plan time).
    pub source_doc: DocumentEndpoint,
    /// Target document endpoint (path + fingerprint at plan time).
    pub target_doc: DocumentEndpoint,
    /// Root task of the transferred subtree (source ID space).
    pub root_task_id: TaskId,
    /// All source task IDs in the subtree, DFS order (root first).
    pub task_ids: Vec<TaskId>,
    /// Source ID → newly allocated target ID.
    pub id_map: Vec<(TaskId, TaskId)>,
    /// Planned asset actions.
    pub asset_refs: Vec<AssetPlanEntry>,
}

impl TransferManifest {
    /// Lookup helper: target ID for a source ID.
    pub fn mapped_id(&self, source_id: &TaskId) -> Option<&TaskId> {
        self.id_map
            .iter()
            .find(|(s, _)| s == source_id)
            .map(|(_, t)| t)
    }

    /// All allocated target IDs.
    pub fn target_ids(&self) -> Vec<&TaskId> {
        self.id_map.iter().map(|(_, t)| t).collect()
    }
}

/// Options for [`plan_transfer`].
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Copy or Move.
    pub operation: TransferOperation,
    /// External dependency handling.
    pub external_dependency_policy: ExternalDependencyPolicy,
    /// Fixed transaction id (tests / retries). Default: fresh UUID v4.
    pub transaction_id: Option<String>,
    /// Source `DocumentId` when the caller knows it.
    pub source_document_id: Option<String>,
    /// Target `DocumentId` when the caller knows it.
    pub target_document_id: Option<String>,
}

/// A fully planned transfer, ready for [`execute_transfer`].
///
/// Holds the rewritten XML clone of the subtree (the lossless payload) plus
/// the manifest and planning warnings.
#[derive(Debug)]
pub struct TransferPlan {
    /// The durable transaction contract.
    pub manifest: TransferManifest,
    /// Non-fatal planning findings.
    pub warnings: Vec<TransferWarning>,
    /// Deep clone of the source `<TASK>` element with IDs, dependencies and
    /// asset refs rewritten for the target document.
    pub subtree_xml: XmlElement,
    /// NEXTUNIQUEID to write into the target root after insertion.
    pub new_target_next_unique_id: u64,
    /// Number of root-level TASK elements in the target before insertion
    /// (used for the POS attribute of the inserted root).
    pub target_root_task_count: usize,
    /// Domain snapshot of the subtree (planning/validation only).
    pub subtree_snapshot: TaskTree,
}

/// Pipeline phase boundaries at which QA hooks can inject a simulated kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    /// Before any side effect (planning already done).
    StageAssets,
    /// After assets staged, before the target temp write/replace.
    CommitTarget,
    /// After target committed, before the source bytes backup (move only).
    BackupSource,
    /// After source backup, before the source rewrite (move only).
    CommitSource,
    /// After both documents committed, before the Completed journal entry.
    Complete,
}

/// Test/QA seams: inject a failure at an exact phase boundary (QA-M8-001~012).
#[derive(Debug, Clone, Default)]
pub struct TransferHooks {
    /// When set, execution aborts with [`TransferError::SimulatedKill`]
    /// immediately BEFORE this phase runs.
    pub fail_before: Option<TransferPhase>,
}

/// Progress events for the transfer UX backend (RD-M8-024~031).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum TransferProgress {
    /// Validating inputs and building the plan.
    Planning,
    /// Copying assets next to the target document.
    StagingAssets { done: usize, total: usize },
    /// Writing + validating + atomically replacing the target document.
    CommittingTarget,
    /// Backing up the source document bytes (move only).
    BackingUpSource,
    /// Removing the subtree from the source document (move only).
    CommittingSource,
    /// Recording completion and cleaning the journal.
    Finishing,
}

/// Result of a successfully executed transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferOutcome {
    /// Transaction id (also the Undo handle).
    pub transaction_id: String,
    /// Copy or Move.
    pub operation: TransferOperation,
    /// Root task ID allocated in the target document.
    pub target_root_task_id: TaskId,
    /// All target IDs inserted (DFS order).
    pub target_task_ids: Vec<TaskId>,
    /// Fingerprint of the target document after commit.
    pub target_fingerprint: FileFingerprint,
    /// Fingerprint of the source document after commit (move only).
    pub source_fingerprint_after: Option<FileFingerprint>,
    /// Warnings collected during planning and execution.
    pub warnings: Vec<TransferWarning>,
    /// True when Undo artifacts were written (see [`undo_transfer`]).
    pub undo_available: bool,
}

/// Structured error for EVERY distinct transfer failure mode (RD-M8-023).
#[derive(Debug, Error)]
pub enum TransferError {
    #[error("cannot read source document {path}: {reason}")]
    SourceRead { path: PathBuf, reason: String },

    #[error("cannot parse source document {path}: {reason}")]
    SourceParse { path: PathBuf, reason: String },

    #[error("task {task_id} not found in source document {path}")]
    SourceTaskNotFound { path: PathBuf, task_id: String },

    #[error("cannot read target document {path}: {reason}")]
    TargetRead { path: PathBuf, reason: String },

    #[error("cannot parse target document {path}: {reason}")]
    TargetParse { path: PathBuf, reason: String },

    #[error("source and target are the same document: {path}")]
    SameDocument { path: PathBuf },

    #[error("transfer blocked: {count} external dependencies (policy=block): {detail}")]
    ExternalDependenciesBlocked { count: usize, detail: String },

    #[error("failed to stage asset {src_path} -> {dst_path}: {reason}")]
    AssetCopyFailed {
        src_path: PathBuf,
        dst_path: PathBuf,
        reason: String,
    },

    #[error("transfer journal error: {0}")]
    Journal(#[from] JournalError),

    #[error("target document changed between planning and commit: {path}")]
    TargetConcurrentlyModified { path: PathBuf },

    #[error(
        "source document changed after target commit; source kept intact (duplicate-not-loss). \
         transaction {transaction_id}: {path}"
    )]
    SourceConcurrentlyModified { path: PathBuf, transaction_id: String },

    #[error("target commit failed for {path}: {code}")]
    TargetCommitFailed { path: PathBuf, code: SaveErrorCode },

    #[error("target validation failed: {reasons:?}")]
    TargetValidationFailed { reasons: Vec<String> },

    #[error("source commit failed for {path}: {code}")]
    SourceCommitFailed { path: PathBuf, code: SaveErrorCode },

    #[error("undo artifacts missing for transaction {transaction_id}")]
    UndoArtifactsMissing { transaction_id: String },

    #[error("undo restore failed for {path}: {reason}")]
    UndoRestoreFailed { path: PathBuf, reason: String },

    #[error("transaction already undone: {transaction_id}")]
    AlreadyUndone { transaction_id: String },

    #[error("simulated kill before phase {phase:?} (QA hook)")]
    SimulatedKill { phase: TransferPhase },

    #[error("recovery precondition failed for {journal_path}: {reason}")]
    RecoveryPrecondition { journal_path: PathBuf, reason: String },

    #[error("I/O error ({context}): {reason}")]
    Io { context: String, reason: String },
}

pub type TransferResult<T> = Result<T, TransferError>;

/// Runtime context for executing / recovering transfers.
#[derive(Debug, Clone)]
pub struct TransferContext {
    /// Directory holding durable journals (`<ws>/.moderntodo/transfer`).
    pub journal_dir: PathBuf,
    /// Directory holding Undo artifacts (`<ws>/.moderntodo/transfer-undo`).
    pub undo_dir: PathBuf,
}

impl TransferContext {
    /// Derives both directories from a workspace root.
    pub fn for_workspace_root(workspace_root: &Path) -> Self {
        Self {
            journal_dir: default_journal_dir(workspace_root),
            undo_dir: workspace_root.join(".moderntodo").join("transfer-undo"),
        }
    }
}

// ─────────────────────────── XML helpers ───────────────────────────

/// Recursively finds the first `<TASK>` element with the given ID attribute.
pub fn find_task_element<'a>(elem: &'a XmlElement, id: &str) -> Option<&'a XmlElement> {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            if child.get_attr("ID") == Some(id) {
                return Some(child);
            }
            if let Some(found) = find_task_element(child, id) {
                return Some(found);
            }
        } else if let Some(found) = find_task_element(child, id) {
            return Some(found);
        }
    }
    None
}

/// Builds a domain [`TaskTree`] snapshot of the subtree rooted at `elem`.
fn snapshot_subtree(elem: &XmlElement) -> TaskTree {
    let mut tree = TaskTree::new();
    fn walk(elem: &XmlElement, tree: &mut TaskTree, is_root: bool) {
        let task = read_task(elem);
        let id = task.id.clone();
        tree.add_task(task);
        if is_root {
            tree.add_root_id(id);
        }
        for child in elem.children_by_tag("TASK") {
            walk(child, tree, false);
        }
    }
    walk(elem, &mut tree, true);
    tree
}

/// Counts `<TASK>` elements recursively.
fn count_task_elements(elem: &XmlElement) -> usize {
    let mut n = 0;
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            n += 1 + count_task_elements(child);
        } else {
            n += count_task_elements(child);
        }
    }
    n
}

/// Collects `<TASK>` element IDs recursively, DFS order.
fn collect_task_ids_dfs(elem: &XmlElement, out: &mut Vec<String>) {
    for child in elem.child_elements() {
        if child.tag == "TASK" {
            if let Some(id) = child.get_attr("ID") {
                out.push(id.to_string());
            }
            collect_task_ids_dfs(child, out);
        }
    }
}

/// Removes the `<TASK>` element with the given ID (and its subtree) from the
/// element tree. Returns true when found and removed.
fn remove_task_element(root: &mut XmlElement, id: &str) -> bool {
    let before = root.children.len();
    let mut new_children = Vec::with_capacity(before);
    let mut removed = false;
    for node in root.children.drain(..) {
        match &node {
            XmlNode::Element(e) if e.tag == "TASK" && e.get_attr("ID") == Some(id) => {
                removed = true; // drop element (and its subtree)
            }
            XmlNode::Element(e) => {
                let mut e = e.clone();
                if remove_task_element(&mut e, id) {
                    removed = true;
                }
                new_children.push(XmlNode::Element(e));
            }
            other => new_children.push(other.clone()),
        }
    }
    root.children = new_children;
    removed
}

fn line_ending(doc: &XmlDocument) -> &'static str {
    doc.meta.line_ending.as_str()
}

/// Appends the transferred subtree clone to the target root, matching the
/// target document's line-ending style and root-level indentation (none).
fn append_subtree_to_root(target: &mut XmlDocument, mut subtree: XmlElement, pos: usize) {
    subtree.set_attr("POS", pos.to_string());
    let le = line_ending(target);
    // Ensure the closing root tag stays on its own line: if the last child is
    // an element (no trailing whitespace text node), add a newline first.
    let needs_leading_newline = match target.root.children.last() {
        Some(XmlNode::Element(_)) => true,
        None => false,
        _ => false,
    };
    if needs_leading_newline {
        target.root.children.push(XmlNode::Text(le.to_string()));
    }
    target.root.children.push(XmlNode::Element(subtree));
    // Trailing newline so `</TODOLIST>` remains on its own line.
    target.root.children.push(XmlNode::Text(le.to_string()));
}

// ─────────────────────────── Asset planning ───────────────────────────

fn is_url_reference(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || lower.starts_with("ftp://")
        || lower.starts_with("mailto:")
}

/// Classifies one raw file reference found inside the subtree.
fn classify_asset(
    owner_task_id: &str,
    raw_ref: &str,
    source_dir: &Path,
    target_dir: &Path,
    warnings: &mut Vec<TransferWarning>,
) -> AssetPlanEntry {
    let trimmed = raw_ref.trim();
    if trimmed.is_empty() {
        return AssetPlanEntry {
            owner_task_id: owner_task_id.to_string(),
            original_ref: raw_ref.to_string(),
            classification: AssetClassification::Skip,
            source_abs: None,
            target_abs: None,
            rewritten_ref: None,
            hash: None,
            reason: Some("empty reference".into()),
        };
    }
    if is_url_reference(trimmed) {
        warnings.push(TransferWarning::AssetUrlKept {
            task_id: owner_task_id.to_string(),
            url: trimmed.to_string(),
        });
        return AssetPlanEntry {
            owner_task_id: owner_task_id.to_string(),
            original_ref: raw_ref.to_string(),
            classification: AssetClassification::ReferenceOnly,
            source_abs: None,
            target_abs: None,
            rewritten_ref: None,
            hash: None,
            reason: Some("URL reference kept verbatim (progress links preserve URLs)".into()),
        };
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        warnings.push(TransferWarning::AssetAbsoluteReferenceKept {
            task_id: owner_task_id.to_string(),
            path: trimmed.to_string(),
        });
        return AssetPlanEntry {
            owner_task_id: owner_task_id.to_string(),
            original_ref: raw_ref.to_string(),
            classification: AssetClassification::ReferenceOnly,
            source_abs: Some(p.to_path_buf()),
            target_abs: None,
            rewritten_ref: None,
            hash: None,
            reason: Some("absolute local path kept verbatim (outside workspace layout)".into()),
        };
    }
    // Document-relative reference: resolve against the source document dir.
    let rel = normalize_relative(trimmed);
    let source_abs = source_dir.join(&rel);
    if !source_abs.is_file() {
        warnings.push(TransferWarning::AssetMissing {
            task_id: owner_task_id.to_string(),
            path: trimmed.to_string(),
        });
        return AssetPlanEntry {
            owner_task_id: owner_task_id.to_string(),
            original_ref: raw_ref.to_string(),
            classification: AssetClassification::Skip,
            source_abs: Some(source_abs),
            target_abs: None,
            rewritten_ref: None,
            hash: None,
            reason: Some("referenced file does not exist; reference kept unchanged".into()),
        };
    }
    let target_abs = target_dir.join(&rel);
    let hash = FileFingerprint::from_file(&source_abs).map(|f| f.hash).ok();

    // Destination collision handling.
    if target_abs.is_file() {
        let dest_hash = FileFingerprint::from_file(&target_abs).map(|f| f.hash).ok();
        if dest_hash.is_some() && dest_hash == hash {
            // Identical file already at destination: nothing to copy, ref unchanged.
            return AssetPlanEntry {
                owner_task_id: owner_task_id.to_string(),
                original_ref: raw_ref.to_string(),
                classification: AssetClassification::Skip,
                source_abs: Some(source_abs),
                target_abs: Some(target_abs),
                rewritten_ref: None,
                hash,
                reason: Some("identical file already present at destination".into()),
            };
        }
        // Different content at destination: collision-safe unique name.
        let unique = unique_sibling_name(&target_abs);
        let new_rel = rel
            .parent()
            .map(|d| d.join(unique.file_name().unwrap()))
            .unwrap_or_else(|| PathBuf::from(unique.file_name().unwrap()));
        warnings.push(TransferWarning::AssetCollisionRenamed {
            from: target_abs.to_string_lossy().into_owned(),
            to: unique.to_string_lossy().into_owned(),
        });
        return AssetPlanEntry {
            owner_task_id: owner_task_id.to_string(),
            original_ref: raw_ref.to_string(),
            classification: AssetClassification::CopyRequired,
            source_abs: Some(source_abs),
            target_abs: Some(unique),
            rewritten_ref: Some(rebuild_ref_style(raw_ref, &new_rel)),
            hash,
            reason: Some("destination collision resolved with unique name".into()),
        };
    }

    AssetPlanEntry {
        owner_task_id: owner_task_id.to_string(),
        original_ref: raw_ref.to_string(),
        classification: AssetClassification::CopyRequired,
        source_abs: Some(source_abs),
        target_abs: Some(target_abs),
        rewritten_ref: None, // same relative layout under the target dir
        hash,
        reason: None,
    }
}

/// Strips leading `.\` / `./` and converts separators for joining.
fn normalize_relative(raw: &str) -> PathBuf {
    let t = raw.trim();
    let t = t.strip_prefix(".\\").or_else(|| t.strip_prefix("./")).unwrap_or(t);
    PathBuf::from(t.replace('/', "\\"))
}

/// Rebuilds a reference string in the same style as the original
/// (`.\x\y.pdf` vs `x/y.pdf`) from a normalized relative path.
fn rebuild_ref_style(original: &str, new_rel: &Path) -> String {
    let s = new_rel.to_string_lossy().replace('/', "\\");
    if original.trim_start().starts_with(".\\") {
        format!(".\\{}", s)
    } else if original.trim_start().starts_with("./") {
        format!("./{}", s.replace('\\', "/"))
    } else {
        s
    }
}

/// Produces a collision-safe sibling path: `<stem>-<uuid8><ext>`.
fn unique_sibling_name(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let short = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    path.with_file_name(format!("{}-{}{}", stem, short, ext))
}

/// Copies one staged asset with hash verification (never overwrites a
/// differing existing file; falls back to a unique sibling name and reports
/// the patched reference).
fn stage_asset(entry: &AssetPlanEntry) -> TransferResult<Option<(String, String)>> {
    let (Some(src), Some(dst)) = (&entry.source_abs, &entry.target_abs) else {
        return Ok(None);
    };
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TransferError::AssetCopyFailed {
            src_path: src.clone(),
            dst_path: dst.clone(),
            reason: e.to_string(),
        })?;
    }
    let mut dst = dst.clone();
    let mut patched: Option<(String, String)> = None; // (planned_ref, actual_ref)
    if dst.is_file() {
        let src_hash = FileFingerprint::from_file(src)
            .map_err(|e| TransferError::AssetCopyFailed {
                src_path: src.clone(),
                dst_path: dst.clone(),
                reason: e.to_string(),
            })?
            .hash;
        let dst_hash = FileFingerprint::from_file(&dst).map(|f| f.hash).unwrap_or_default();
        if dst_hash == src_hash {
            // Already staged (e.g. retry after a crash before target commit).
            return Ok(None);
        }
        // External change since planning: never clobber, use a unique name.
        let unique = unique_sibling_name(&dst);
        let planned_ref = entry.rewritten_ref.clone().unwrap_or_else(|| entry.original_ref.clone());
        let actual_rel = unique
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let actual_ref = rebuild_ref_style(&planned_ref, Path::new(&actual_rel));
        patched = Some((planned_ref, actual_ref));
        dst = unique;
    }
    std::fs::copy(src, &dst).map_err(|e| TransferError::AssetCopyFailed {
        src_path: src.clone(),
        dst_path: dst.clone(),
        reason: e.to_string(),
    })?;
    // Verify the copy.
    if let Some(expected) = &entry.hash {
        let actual = FileFingerprint::from_file(&dst)
            .map_err(|e| TransferError::AssetCopyFailed {
                src_path: src.clone(),
                dst_path: dst.clone(),
                reason: e.to_string(),
            })?
            .hash;
        if &actual != expected {
            let _ = std::fs::remove_file(&dst);
            return Err(TransferError::AssetCopyFailed {
                src_path: src.clone(),
                dst_path: dst,
                reason: "hash mismatch after copy".into(),
            });
        }
    }
    Ok(patched)
}

/// Replaces every FILEREFPATH text equal to `old_ref` with `new_ref` inside
/// the subtree clone (used when staging had to rename past the plan).
fn patch_asset_ref(subtree: &mut XmlElement, old_ref: &str, new_ref: &str) {
    for node in subtree.children.iter_mut() {
        match node {
            XmlNode::Element(e) => {
                if e.tag == "FILEREFPATH" && e.text_content().trim() == old_ref.trim() {
                    e.set_text_content(new_ref.to_string());
                } else {
                    patch_asset_ref(e, old_ref, new_ref);
                }
            }
            XmlNode::Text(t) if t.trim() == old_ref.trim() => {
                // text directly under a FILEREFPATH is handled above; guard
                // against bare text equality only inside rewritten elements.
                let _ = t;
            }
            _ => {}
        }
    }
}

// ─────────────────────────── Subtree rewriting ───────────────────────────

/// Rewrites the cloned subtree in place:
/// - every nested `<TASK ID>` through the id map,
/// - internal `<DEPENDENCY><TASKID>` refs through the id map,
/// - external deps per policy (adds `<FILENAME>` marker for PreserveAsExternal),
/// - `<FILEREFPATH>` texts per the asset plan,
/// - HTML comment bodies: copied-asset relative refs string-replaced.
fn rewrite_subtree(
    subtree: &mut XmlElement,
    id_map: &HashMap<String, String>,
    asset_rewrites: &[(String, String)],
    external_policy: ExternalDependencyPolicy,
    source_doc_name: &str,
    warnings: &mut Vec<TransferWarning>,
) {
    // Original (source-space) ID for warnings, then TASK id rewrite.
    let original_id = subtree.get_attr("ID").map(|s| s.to_string());
    if let Some(id) = &original_id {
        if let Some(new_id) = id_map.get(id) {
            subtree.set_attr("ID", new_id.as_str());
        }
    }
    let mut children_to_recurse: Vec<usize> = Vec::new();
    for (idx, node) in subtree.children.iter_mut().enumerate() {
        if let XmlNode::Element(e) = node {
            match e.tag.as_str() {
                "TASK" => {
                    children_to_recurse.push(idx);
                }
                "DEPENDENCY" => {
                    rewrite_dependency(e, id_map, external_policy, source_doc_name, warnings,
                        original_id.clone());
                }
                "FILEREFPATH" => {
                    let text = e.text_content();
                    if let Some((_, new_ref)) = asset_rewrites
                        .iter()
                        .find(|(old, _)| old.trim() == text.trim())
                    {
                        e.set_text_content(new_ref.clone());
                    }
                }
                "COMMENTS" => {
                    rewrite_comment_paths(e, asset_rewrites);
                }
                _ => {
                    // Unknown elements may nest references we do not model;
                    // recurse conservatively for DEPENDENCY-like structures.
                    rewrite_unknown_deep(e, id_map, asset_rewrites);
                }
            }
        }
    }
    for idx in children_to_recurse {
        if let Some(XmlNode::Element(e)) = subtree.children.get_mut(idx) {
            rewrite_subtree(e, id_map, asset_rewrites, external_policy, source_doc_name, warnings);
        }
    }
}

fn rewrite_dependency(
    dep: &mut XmlElement,
    id_map: &HashMap<String, String>,
    external_policy: ExternalDependencyPolicy,
    source_doc_name: &str,
    warnings: &mut Vec<TransferWarning>,
    owner_task_id: Option<String>,
) {
    let taskid_elem = dep
        .children
        .iter_mut()
        .find_map(|n| match n {
            XmlNode::Element(e) if e.tag == "TASKID" => Some(e),
            _ => None,
        });
    let Some(taskid_elem) = taskid_elem else { return };
    let raw = taskid_elem.text_content();
    let key = raw.trim().to_string();
    if let Some(new_id) = id_map.get(&key) {
        // Internal dependency: both endpoints inside the subtree.
        taskid_elem.set_text_content(new_id.clone());
    } else if !key.is_empty() {
        // External dependency (endpoint outside the subtree).
        match external_policy {
            ExternalDependencyPolicy::PreserveAsExternal => {
                // Keep the numeric TASKID, add a FILENAME marker so the ref is
                // interpretable as cross-document (see policy doc §3).
                let already_marked = dep
                    .child_elements()
                    .any(|c| c.tag == "FILENAME");
                if !already_marked {
                    let mut fname = XmlElement::new("FILENAME");
                    fname.set_text_content(source_doc_name.to_string());
                    dep.children.push(XmlNode::Element(fname));
                }
                warnings.push(TransferWarning::ExternalDependencyPreserved {
                    task_id: owner_task_id.unwrap_or_default(),
                    dep_task_id: key,
                    source_doc: source_doc_name.to_string(),
                });
            }
            ExternalDependencyPolicy::Block => {
                // Handled during planning (plan aborts); nothing to do here.
            }
        }
    }
}

/// String-replaces copied-asset refs inside comment text (HTML `<img src>` /
/// links). Only exact ref strings that were physically copied are rewritten;
/// URLs are never touched.
fn rewrite_comment_paths(comments: &mut XmlElement, asset_rewrites: &[(String, String)]) {
    fn walk(node: &mut XmlNode, rewrites: &[(String, String)]) {
        match node {
            XmlNode::Text(t) => {
                let mut out = t.clone();
                for (old, new) in rewrites {
                    if out.contains(old.as_str()) {
                        out = out.replace(old.as_str(), new);
                    }
                }
                *t = out;
            }
            XmlNode::CData(t) => {
                let mut out = t.clone();
                for (old, new) in rewrites {
                    if out.contains(old.as_str()) {
                        out = out.replace(old.as_str(), new);
                    }
                }
                *t = out;
            }
            XmlNode::Element(e) => {
                for attr in e.attrs.iter_mut() {
                    for (old, new) in rewrites {
                        if attr.value.contains(old.as_str()) {
                            attr.value = attr.value.replace(old.as_str(), new);
                        }
                    }
                }
                for c in e.children.iter_mut() {
                    walk(c, rewrites);
                }
            }
            _ => {}
        }
    }
    for node in comments.children.iter_mut() {
        walk(node, asset_rewrites);
    }
}

/// Conservative rewrite inside unknown elements: only TEXT of elements named
/// TASKID and FILEREFPATH-like refs, mirroring the known-element rules.
fn rewrite_unknown_deep(elem: &mut XmlElement, id_map: &HashMap<String, String>, asset_rewrites: &[(String, String)]) {
    for node in elem.children.iter_mut() {
        if let XmlNode::Element(e) = node {
            match e.tag.as_str() {
                "TASKID" => {
                    let key = e.text_content().trim().to_string();
                    if let Some(new_id) = id_map.get(&key) {
                        e.set_text_content(new_id.clone());
                    }
                }
                "TASK" => {
                    if let Some(id) = e.get_attr("ID").map(|s| s.to_string()) {
                        if let Some(new_id) = id_map.get(&id) {
                            e.set_attr("ID", new_id.as_str());
                        }
                    }
                    rewrite_unknown_deep(e, id_map, asset_rewrites);
                }
                "FILEREFPATH" => {
                    let text = e.text_content();
                    if let Some((_, new_ref)) = asset_rewrites
                        .iter()
                        .find(|(old, _)| old.trim() == text.trim())
                    {
                        e.set_text_content(new_ref.clone());
                    }
                }
                _ => rewrite_unknown_deep(e, id_map, asset_rewrites),
            }
        }
    }
}

// ─────────────────────────── Planning ───────────────────────────

/// Plans a transfer of the subtree rooted at `root_task_id` from
/// `source_path` into `target_path`.
///
/// Planning is side-effect free: on any error nothing has been written.
pub fn plan_transfer(
    source_path: &Path,
    target_path: &Path,
    root_task_id: &TaskId,
    options: &PlanOptions,
) -> TransferResult<TransferPlan> {
    if same_file(source_path, target_path) {
        return Err(TransferError::SameDocument {
            path: source_path.to_path_buf(),
        });
    }

    // ── Read + parse source ──
    let source_bytes = std::fs::read(source_path).map_err(|e| TransferError::SourceRead {
        path: source_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let source_fp = FileFingerprint::from_bytes(&source_bytes);
    let source_doc = parse_xml(&source_bytes).map_err(|e| TransferError::SourceParse {
        path: source_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let subtree_src = find_task_element(&source_doc.root, root_task_id.as_str()).ok_or(
        TransferError::SourceTaskNotFound {
            path: source_path.to_path_buf(),
            task_id: root_task_id.as_str().to_string(),
        },
    )?;

    // ── Read + parse target ──
    let target_bytes = std::fs::read(target_path).map_err(|e| TransferError::TargetRead {
        path: target_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let target_fp = FileFingerprint::from_bytes(&target_bytes);
    let target_doc = parse_xml(&target_bytes).map_err(|e| TransferError::TargetParse {
        path: target_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // ── Domain snapshot of the subtree ──
    let subtree_snapshot = snapshot_subtree(subtree_src);
    if subtree_snapshot.is_empty() {
        return Err(TransferError::SourceTaskNotFound {
            path: source_path.to_path_buf(),
            task_id: root_task_id.as_str().to_string(),
        });
    }
    let mut warnings = Vec::new();
    let mut subtree_ids_dfs: Vec<String> = Vec::new();
    // The subtree ROOT id first, then descendants in DFS order.
    if let Some(id) = subtree_src.get_attr("ID") {
        subtree_ids_dfs.push(id.to_string());
    }
    collect_task_ids_dfs(subtree_src, &mut subtree_ids_dfs);
    let subtree_id_set: HashSet<String> = subtree_ids_dfs.iter().cloned().collect();
    for id in &subtree_ids_dfs {
        if id.parse::<u64>().is_err() {
            warnings.push(TransferWarning::NonNumericTaskId { id: id.clone() });
        }
    }

    // ── Target ID space (docs/compatibility/ID_ALLOCATION_RULES.md) ──
    let mut target_tree = TaskTree::new();
    let mut target_ids_all: Vec<String> = Vec::new();
    collect_task_ids_dfs(&target_doc.root, &mut target_ids_all);
    for id in &target_ids_all {
        target_tree.add_task(Task::new(TaskId::new(id.as_str())));
    }
    let declared_next = target_doc
        .root
        .get_attr("NEXTUNIQUEID")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    // Rule §5.3-4: if max(ID) >= NEXTUNIQUEID, correct to max(ID)+1.
    let max_numeric = target_ids_all
        .iter()
        .filter_map(|s| s.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let effective_next = declared_next.max(max_numeric + 1);
    let mut allocator = TaskIdAllocator::new(&target_tree, effective_next);

    let mut id_map_pairs: Vec<(TaskId, TaskId)> = Vec::new();
    let mut id_map: HashMap<String, String> = HashMap::new();
    for id in &subtree_ids_dfs {
        let new_id = allocator.allocate();
        id_map.insert(id.clone(), new_id.as_str().to_string());
        id_map_pairs.push((TaskId::new(id.as_str()), new_id));
    }
    let new_target_next_unique_id = allocator.next_unique_id();

    // ── Dependency classification + policy gate ──
    let mut external_found: Vec<(String, String)> = Vec::new();
    let mut orphaned_source_deps_after_move: Vec<(String, String)> = Vec::new();
    for task in subtree_snapshot.iter() {
        for dep in &task.dependencies {
            let key = dep.task_id.trim();
            if key.is_empty() {
                continue;
            }
            if !subtree_id_set.contains(key) {
                external_found.push((task.id.as_str().to_string(), key.to_string()));
            }
        }
    }
    if options.operation == TransferOperation::Move {
        // Deps from OUTSIDE tasks onto moved tasks become unresolved in the
        // source after deletion (preserved, warned — never silently dropped).
        let moved: HashSet<&str> = subtree_ids_dfs.iter().map(|s| s.as_str()).collect();
        let mut all_source: Vec<Task> = Vec::new();
        fn collect_tasks(elem: &XmlElement, out: &mut Vec<Task>) {
            for child in elem.child_elements() {
                if child.tag == "TASK" {
                    out.push(read_task(child));
                    collect_tasks(child, out);
                }
            }
        }
        collect_tasks(&source_doc.root, &mut all_source);
        for t in &all_source {
            if moved.contains(t.id.as_str()) {
                continue;
            }
            for dep in &t.dependencies {
                if moved.contains(dep.task_id.trim()) {
                    orphaned_source_deps_after_move.push((
                        t.id.as_str().to_string(),
                        dep.task_id.trim().to_string(),
                    ));
                }
            }
        }
    }
    if !external_found.is_empty()
        && options.external_dependency_policy == ExternalDependencyPolicy::Block
    {
        let detail = external_found
            .iter()
            .map(|(t, d)| format!("task {} -> {}", t, d))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(TransferError::ExternalDependenciesBlocked {
            count: external_found.len(),
            detail,
        });
    }

    // ── Asset plan ──
    let source_dir = source_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let target_dir = target_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut asset_refs: Vec<AssetPlanEntry> = Vec::new();
    for task in subtree_snapshot.iter() {
        for link in &task.file_links {
            asset_refs.push(classify_asset(
                task.id.as_str(),
                &link.path,
                &source_dir,
                &target_dir,
                &mut warnings,
            ));
        }
        // HTML comments may embed managed image refs (M7 rich text).
        if let Some(c) = &task.comments {
            if matches!(c.comment_type, CommentType::Html) {
                for rel in extract_relative_refs_from_html(&c.content) {
                    // Avoid duplicating refs already planned via FILEREFPATH.
                    if asset_refs.iter().any(|a| a.original_ref.trim() == rel.trim()) {
                        continue;
                    }
                    asset_refs.push(classify_asset(
                        task.id.as_str(),
                        &rel,
                        &source_dir,
                        &target_dir,
                        &mut warnings,
                    ));
                }
            }
        }
    }
    for (t, d) in orphaned_source_deps_after_move {
        warnings.push(TransferWarning::OrphanedSourceDependency {
            task_id: t,
            dep_task_id: d,
        });
    }

    // ── Logical clone: deep-clone the XML subtree and rewrite references ──
    let mut subtree_xml = subtree_src.clone();
    let asset_rewrites: Vec<(String, String)> = asset_refs
        .iter()
        .filter(|a| a.classification == AssetClassification::CopyRequired)
        .filter_map(|a| {
            a.rewritten_ref
                .clone()
                .map(|r| (a.original_ref.clone(), r))
        })
        .collect();
    let source_doc_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_path.to_string_lossy().into_owned());
    rewrite_subtree(
        &mut subtree_xml,
        &id_map,
        &asset_rewrites,
        options.external_dependency_policy,
        &source_doc_name,
        &mut warnings,
    );

    let target_root_task_count = target_doc.root.children_by_tag("TASK").count();

    let transaction_id = options
        .transaction_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let manifest = TransferManifest {
        transaction_id,
        operation: options.operation,
        source_doc: DocumentEndpoint {
            document_id: options.source_document_id.clone(),
            path: source_path.to_path_buf(),
            fingerprint: source_fp,
        },
        target_doc: DocumentEndpoint {
            document_id: options.target_document_id.clone(),
            path: target_path.to_path_buf(),
            fingerprint: target_fp,
        },
        root_task_id: root_task_id.clone(),
        task_ids: subtree_ids_dfs
            .iter()
            .map(|s| TaskId::new(s.as_str()))
            .collect(),
        id_map: id_map_pairs,
        asset_refs,
    };

    Ok(TransferPlan {
        manifest,
        warnings,
        subtree_xml,
        new_target_next_unique_id,
        target_root_task_count,
        subtree_snapshot,
    })
}

/// Extracts candidate relative file refs from HTML comment content
/// (`src="..."` / `href="..."` values that are document-relative paths).
fn extract_relative_refs_from_html(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["src=\"", "href=\"", "src='", "href='"] {
        let quote = if marker.ends_with('"') { '"' } else { '\'' };
        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find(marker) {
            let start = search_from + pos + marker.len();
            let rest = &html[start..];
            match rest.find(quote) {
                Some(end) => {
                    let value = &rest[..end];
                    search_from = start + end + 1;
                    let trimmed = value.trim();
                    if !trimmed.is_empty()
                        && !is_url_reference(trimmed)
                        && !Path::new(trimmed).is_absolute()
                        && !trimmed.starts_with('#')
                        && !trimmed.starts_with("mailto:")
                    {
                        if !out.iter().any(|s: &String| s == trimmed) {
                            out.push(trimmed.to_string());
                        }
                    }
                }
                None => break,
            }
        }
    }
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

// ─────────────────────────── Execution ───────────────────────────

/// Executes a planned transfer (copy or move) with the target-first protocol.
///
/// `progress` receives UX progress events; `hooks` allows QA to inject a
/// simulated kill at an exact phase boundary.
pub fn execute_transfer(
    mut plan: TransferPlan,
    ctx: &TransferContext,
    hooks: &TransferHooks,
    mut progress: impl FnMut(TransferProgress),
) -> TransferResult<TransferOutcome> {
    let manifest = &plan.manifest;

    // ── Phase: StageAssets ──
    check_hook(hooks, TransferPhase::StageAssets)?;
    progress(TransferProgress::Planning);

    // Revalidate both fingerprints before any side effect.
    let source_bytes_now =
        std::fs::read(&manifest.source_doc.path).map_err(|e| TransferError::SourceRead {
            path: manifest.source_doc.path.clone(),
            reason: e.to_string(),
        })?;
    if !manifest.source_doc.fingerprint.matches_bytes(&source_bytes_now) {
        return Err(TransferError::SourceConcurrentlyModified {
            path: manifest.source_doc.path.clone(),
            transaction_id: manifest.transaction_id.clone(),
        });
    }
    let target_bytes_now =
        std::fs::read(&manifest.target_doc.path).map_err(|e| TransferError::TargetRead {
            path: manifest.target_doc.path.clone(),
            reason: e.to_string(),
        })?;
    if !manifest.target_doc.fingerprint.matches_bytes(&target_bytes_now) {
        return Err(TransferError::TargetConcurrentlyModified {
            path: manifest.target_doc.path.clone(),
        });
    }

    // Durable journal: Started (before the first side effect: asset staging).
    let txn_dir = ctx.undo_dir.join(&manifest.transaction_id);
    std::fs::create_dir_all(&txn_dir).map_err(|e| TransferError::Io {
        context: "create undo dir".into(),
        reason: e.to_string(),
    })?;
    let target_undo_backup = txn_dir.join("target.before.xml");
    let mut journal = TransferJournal::create(
        &ctx.journal_dir,
        manifest.clone(),
        Some(target_undo_backup.clone()),
    )?;

    // Stage assets.
    let copy_entries: Vec<AssetPlanEntry> = manifest
        .asset_refs
        .iter()
        .filter(|a| a.classification == AssetClassification::CopyRequired)
        .cloned()
        .collect();
    let total_assets = copy_entries.len();
    for (i, entry) in copy_entries.iter().enumerate() {
        if let Some((planned_ref, actual_ref)) = stage_asset(entry)? {
            patch_asset_ref(&mut plan.subtree_xml, &planned_ref, &actual_ref);
        }
        progress(TransferProgress::StagingAssets {
            done: i + 1,
            total: total_assets,
        });
    }
    journal.append(JournalPhase::AssetsStaged, None)?;

    // ── Phase: CommitTarget ──
    check_hook(hooks, TransferPhase::CommitTarget)?;
    progress(TransferProgress::CommittingTarget);

    // Undo artifact: exact pre-commit target bytes.
    write_durable(&target_undo_backup, &target_bytes_now)?;

    // Build the new target document from the freshly read bytes.
    let mut target_doc = parse_xml(&target_bytes_now).map_err(|e| TransferError::TargetParse {
        path: manifest.target_doc.path.clone(),
        reason: e.to_string(),
    })?;
    let expected_task_count =
        count_task_elements(&target_doc.root) + manifest.task_ids.len();
    append_subtree_to_root(
        &mut target_doc,
        plan.subtree_xml.clone(),
        plan.target_root_task_count,
    );
    target_doc
        .root
        .set_attr("NEXTUNIQUEID", plan.new_target_next_unique_id.to_string());
    let new_target_bytes = serialize_xml(&target_doc);

    let expected_ids: Vec<String> = manifest
        .id_map
        .iter()
        .map(|(_, t)| t.as_str().to_string())
        .collect();
    let validate_reasons = validate_target_bytes(
        &new_target_bytes,
        &expected_ids,
        expected_task_count,
        plan.new_target_next_unique_id,
    );
    if !validate_reasons.is_empty() {
        return Err(TransferError::TargetValidationFailed {
            reasons: validate_reasons,
        });
    }

    let save = atomic_save(
        &SaveConfig {
            target_path: manifest.target_doc.path.clone(),
            validate_temp: true,
        },
        &new_target_bytes,
        |bytes| validate_target_bytes(bytes, &expected_ids, expected_task_count, plan.new_target_next_unique_id).is_empty(),
    );
    if save.state != super::session::SaveState::Completed {
        return Err(TransferError::TargetCommitFailed {
            path: manifest.target_doc.path.clone(),
            code: save.error.unwrap_or(SaveErrorCode::ReplacementFailed),
        });
    }
    let target_fp_after = save
        .fingerprint
        .unwrap_or_else(|| FileFingerprint::from_bytes(&new_target_bytes));

    // Durable proof that the target holds the subtree. This entry authorizes
    // every later destructive source step.
    journal.append(
        JournalPhase::TargetCommitted,
        Some(format!("target_fp={}", target_fp_after.hash)),
    )?;

    let mut source_fp_after: Option<FileFingerprint> = None;

    if manifest.operation == TransferOperation::Move {
        // ── Phase: BackupSource ──
        check_hook(hooks, TransferPhase::BackupSource)?;
        progress(TransferProgress::BackingUpSource);
        let source_backup = txn_dir.join("source.before.xml");
        write_durable(&source_backup, &source_bytes_now)?;
        journal.append_source_backed_up(&source_backup)?;

        // ── Phase: CommitSource ──
        check_hook(hooks, TransferPhase::CommitSource)?;
        progress(TransferProgress::CommittingSource);
        source_fp_after = Some(commit_source_deletion(&mut journal, &plan)?);
    }

    // ── Phase: Complete ──
    check_hook(hooks, TransferPhase::Complete)?;
    progress(TransferProgress::Finishing);
    write_undo_record(ctx, &plan, source_fp_after.is_some())?;
    journal.append(JournalPhase::Completed, None)?;
    journal.remove();

    Ok(TransferOutcome {
        transaction_id: manifest.transaction_id.clone(),
        operation: manifest.operation,
        target_root_task_id: manifest
            .mapped_id(&manifest.root_task_id)
            .cloned()
            .unwrap_or_else(|| manifest.root_task_id.clone()),
        target_task_ids: manifest.target_ids().into_iter().cloned().collect(),
        target_fingerprint: target_fp_after,
        source_fingerprint_after: source_fp_after,
        warnings: std::mem::take(&mut plan.warnings),
        undo_available: true,
    })
}

fn check_hook(hooks: &TransferHooks, phase: TransferPhase) -> TransferResult<()> {
    if hooks.fail_before == Some(phase) {
        Err(TransferError::SimulatedKill { phase })
    } else {
        Ok(())
    }
}

fn write_durable(path: &Path, bytes: &[u8]) -> TransferResult<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TransferError::Io {
            context: "create backup dir".into(),
            reason: e.to_string(),
        })?;
    }
    let mut f = std::fs::File::create(path).map_err(|e| TransferError::Io {
        context: format!("write {}", path.display()),
        reason: e.to_string(),
    })?;
    f.write_all(bytes).map_err(|e| TransferError::Io {
        context: format!("write {}", path.display()),
        reason: e.to_string(),
    })?;
    f.sync_all().map_err(|e| TransferError::Io {
        context: format!("fsync {}", path.display()),
        reason: e.to_string(),
    })?;
    Ok(())
}

/// Validates candidate target bytes: well-formed, all mapped IDs present
/// exactly once, expected task count, NEXTUNIQUEID above every numeric ID,
/// no unresolved internal dependency (a DEPENDENCY TASKID must resolve inside
/// the document or carry a FILENAME external marker).
fn validate_target_bytes(
    bytes: &[u8],
    expected_ids: &[String],
    expected_task_count: usize,
    expected_next_unique_id: u64,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let doc = match parse_xml(bytes) {
        Ok(d) => d,
        Err(e) => return vec![format!("re-parse failed: {}", e)],
    };
    let mut all_ids: Vec<String> = Vec::new();
    collect_task_ids_dfs(&doc.root, &mut all_ids);
    if all_ids.len() != expected_task_count {
        reasons.push(format!(
            "task count {} != expected {}",
            all_ids.len(),
            expected_task_count
        ));
    }
    let id_set: HashSet<&str> = all_ids.iter().map(|s| s.as_str()).collect();
    for id in expected_ids {
        let occurrences = all_ids.iter().filter(|x| *x == id).count();
        if occurrences != 1 {
            reasons.push(format!("transferred id {} occurs {} times", id, occurrences));
        }
    }
    let nuid = doc
        .root
        .get_attr("NEXTUNIQUEID")
        .and_then(|v| v.parse::<u64>().ok());
    match nuid {
        Some(v) if v == expected_next_unique_id => {}
        Some(v) => reasons.push(format!(
            "NEXTUNIQUEID {} != expected {}",
            v, expected_next_unique_id
        )),
        None => reasons.push("NEXTUNIQUEID missing/unparseable".into()),
    }
    let max_numeric = all_ids
        .iter()
        .filter_map(|s| s.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    if nuid.unwrap_or(0) <= max_numeric {
        reasons.push("NEXTUNIQUEID not greater than max task ID".into());
    }
    // Dependency resolution check.
    fn walk_deps(elem: &XmlElement, id_set: &HashSet<&str>, reasons: &mut Vec<String>) {
        for child in elem.child_elements() {
            match child.tag.as_str() {
                "DEPENDENCY" => {
                    let taskid = child
                        .first_child_by_tag("TASKID")
                        .map(|t| t.text_content().trim().to_string())
                        .unwrap_or_default();
                    let has_filename = child.first_child_by_tag("FILENAME").is_some();
                    if !taskid.is_empty() && !id_set.contains(taskid.as_str()) && !has_filename {
                        reasons.push(format!(
                            "unresolved dependency ref {} without FILENAME marker",
                            taskid
                        ));
                    }
                }
                "TASK" => walk_deps(child, id_set, reasons),
                _ => walk_deps(child, id_set, reasons),
            }
        }
    }
    walk_deps(&doc.root, &id_set, &mut reasons);
    reasons
}

/// The SOURCE DELETION PHASE (RD-M8-015~017).
///
/// Runs only when the journal durably proves `TargetCommitted`. Re-reads and
/// re-fingerprints the source to detect concurrent external edits, removes the
/// subtree, validates, and atomically replaces the source document. Returns
/// the post-commit source fingerprint.
fn commit_source_deletion(
    journal: &mut TransferJournal,
    plan: &TransferPlan,
) -> TransferResult<FileFingerprint> {
    let manifest = &plan.manifest;
    // Hard gate: never touch the source without durable proof of target commit.
    if !journal.record().has_phase(JournalPhase::TargetCommitted) {
        return Err(TransferError::RecoveryPrecondition {
            journal_path: journal.path().to_path_buf(),
            reason: "TargetCommitted journal entry not present".into(),
        });
    }
    let source_bytes =
        std::fs::read(&manifest.source_doc.path).map_err(|e| TransferError::SourceRead {
            path: manifest.source_doc.path.clone(),
            reason: e.to_string(),
        })?;
    if !manifest.source_doc.fingerprint.matches_bytes(&source_bytes) {
        // Concurrent external edit detected: abort BEFORE deleting anything.
        // Target is already committed, so the safe outcome is a duplicate.
        return Err(TransferError::SourceConcurrentlyModified {
            path: manifest.source_doc.path.clone(),
            transaction_id: manifest.transaction_id.clone(),
        });
    }
    let mut source_doc = parse_xml(&source_bytes).map_err(|e| TransferError::SourceParse {
        path: manifest.source_doc.path.clone(),
        reason: e.to_string(),
    })?;
    let count_before = count_task_elements(&source_doc.root);
    let expected_after = count_before
        .checked_sub(plan.subtree_snapshot.len())
        .ok_or_else(|| TransferError::RecoveryPrecondition {
            journal_path: journal.path().to_path_buf(),
            reason: "subtree larger than source document".into(),
        })?;
    if !remove_task_element(&mut source_doc.root, manifest.root_task_id.as_str()) {
        return Err(TransferError::SourceTaskNotFound {
            path: manifest.source_doc.path.clone(),
            task_id: manifest.root_task_id.as_str().to_string(),
        });
    }
    let new_source_bytes = serialize_xml(&source_doc);
    let removed_ids: Vec<String> = manifest
        .task_ids
        .iter()
        .map(|t| t.as_str().to_string())
        .collect();
    let validate = |bytes: &[u8]| -> Vec<String> {
        let mut reasons = Vec::new();
        match parse_xml(bytes) {
            Ok(doc) => {
                let mut ids: Vec<String> = Vec::new();
                collect_task_ids_dfs(&doc.root, &mut ids);
                if ids.len() != expected_after {
                    reasons.push(format!(
                        "source task count {} != expected {}",
                        ids.len(),
                        expected_after
                    ));
                }
                let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
                for rid in &removed_ids {
                    if id_set.contains(rid.as_str()) {
                        reasons.push(format!("removed id {} still present", rid));
                    }
                }
            }
            Err(e) => reasons.push(format!("source re-parse failed: {}", e)),
        }
        reasons
    };
    let save = atomic_save(
        &SaveConfig {
            target_path: manifest.source_doc.path.clone(),
            validate_temp: true,
        },
        &new_source_bytes,
        |bytes| validate(bytes).is_empty(),
    );
    if save.state != super::session::SaveState::Completed {
        return Err(TransferError::SourceCommitFailed {
            path: manifest.source_doc.path.clone(),
            code: save.error.unwrap_or(SaveErrorCode::ReplacementFailed),
        });
    }
    journal.append(JournalPhase::SourceCommitted, None)?;
    Ok(save
        .fingerprint
        .unwrap_or_else(|| FileFingerprint::from_bytes(&new_source_bytes)))
}

/// Convenience service: copy a task subtree between documents (RD-M8-020).
/// Never modifies the source document.
pub fn copy_task(
    source_path: &Path,
    target_path: &Path,
    root_task_id: &TaskId,
    options: &PlanOptions,
    ctx: &TransferContext,
    progress: impl FnMut(TransferProgress),
) -> TransferResult<TransferOutcome> {
    let mut opts = options.clone();
    opts.operation = TransferOperation::Copy;
    let plan = plan_transfer(source_path, target_path, root_task_id, &opts)?;
    execute_transfer(plan, ctx, &TransferHooks::default(), progress)
}

/// Convenience service: move a task subtree between documents (RD-M8-021).
/// Target-first commit + guarded source-delete phase.
pub fn move_task(
    source_path: &Path,
    target_path: &Path,
    root_task_id: &TaskId,
    options: &PlanOptions,
    ctx: &TransferContext,
    progress: impl FnMut(TransferProgress),
) -> TransferResult<TransferOutcome> {
    let mut opts = options.clone();
    opts.operation = TransferOperation::Move;
    let plan = plan_transfer(source_path, target_path, root_task_id, &opts)?;
    execute_transfer(plan, ctx, &TransferHooks::default(), progress)
}

// ─────────────────────────── Undo (RD-M8-022) ───────────────────────────

/// Durable Undo record for one completed transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferUndoRecord {
    pub transaction_id: String,
    pub operation: TransferOperation,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    /// Exact pre-commit target bytes.
    pub target_backup: PathBuf,
    /// Exact pre-commit source bytes (move only).
    pub source_backup: Option<PathBuf>,
    /// NEXTUNIQUEID written at commit; restored-on-undo value must not go
    /// below this (IDs are never recycled — ID_ALLOCATION_RULES §5.3-3).
    pub target_next_unique_id_after: u64,
    pub undone: bool,
}

fn write_undo_record(
    ctx: &TransferContext,
    plan: &TransferPlan,
    has_source_backup: bool,
) -> TransferResult<()> {
    let txn_dir = ctx.undo_dir.join(&plan.manifest.transaction_id);
    let record = TransferUndoRecord {
        transaction_id: plan.manifest.transaction_id.clone(),
        operation: plan.manifest.operation,
        source_path: plan.manifest.source_doc.path.clone(),
        target_path: plan.manifest.target_doc.path.clone(),
        target_backup: txn_dir.join("target.before.xml"),
        source_backup: if has_source_backup {
            Some(txn_dir.join("source.before.xml"))
        } else {
            None
        },
        target_next_unique_id_after: plan.new_target_next_unique_id,
        undone: false,
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| TransferError::Io {
        context: "serialize undo record".into(),
        reason: e.to_string(),
    })?;
    write_durable(&undo_record_path(ctx, &record.transaction_id), &bytes)?;
    Ok(())
}

fn undo_record_path(ctx: &TransferContext, txn: &str) -> PathBuf {
    ctx.undo_dir.join(txn).join("undo.json")
}

/// Loads the Undo record for a completed transaction.
pub fn load_undo_record(
    ctx: &TransferContext,
    transaction_id: &str,
) -> TransferResult<TransferUndoRecord> {
    let path = undo_record_path(ctx, transaction_id);
    let bytes = std::fs::read(&path).map_err(|_| TransferError::UndoArtifactsMissing {
        transaction_id: transaction_id.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|e| TransferError::UndoArtifactsMissing {
        transaction_id: format!("{} (corrupt undo record: {})", transaction_id, e),
    })
}

/// Transaction-level Undo (RD-M8-022):
/// - completed Copy → removes the inserted subtree from the target,
/// - completed Move → additionally restores the source subtree.
///
/// Restores are byte-exact backups committed via the atomic-save pipeline.
/// NEXTUNIQUEID never regresses (IDs are not recycled).
pub fn undo_transfer(ctx: &TransferContext, transaction_id: &str) -> TransferResult<()> {
    let mut record = load_undo_record(ctx, transaction_id)?;
    if record.undone {
        return Err(TransferError::AlreadyUndone {
            transaction_id: transaction_id.to_string(),
        });
    }
    let target_backup_bytes = std::fs::read(&record.target_backup).map_err(|_| {
        TransferError::UndoArtifactsMissing {
            transaction_id: transaction_id.to_string(),
        }
    })?;

    // Restore target first (removes the transferred subtree from it).
    let restored_target = bump_next_unique_id(
        &target_backup_bytes,
        record.target_next_unique_id_after,
        &record.target_path,
    )?;
    atomic_restore(&record.target_path, &restored_target)?;

    // Then restore source (move only): re-inserts the subtree at origin.
    if let Some(sb) = &record.source_backup {
        let source_backup_bytes =
            std::fs::read(sb).map_err(|_| TransferError::UndoArtifactsMissing {
                transaction_id: transaction_id.to_string(),
            })?;
        atomic_restore(&record.source_path, &source_backup_bytes)?;
    }

    record.undone = true;
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| TransferError::Io {
        context: "serialize undo record".into(),
        reason: e.to_string(),
    })?;
    write_durable(&undo_record_path(ctx, transaction_id), &bytes)?;
    Ok(())
}

/// Restores exact bytes atomically (temp + fsync + validated replace).
fn atomic_restore(path: &Path, bytes: &[u8]) -> TransferResult<()> {
    let save = atomic_save(
        &SaveConfig {
            target_path: path.to_path_buf(),
            validate_temp: true,
        },
        bytes,
        |b| parse_xml(b).is_ok(),
    );
    if save.state != super::session::SaveState::Completed {
        return Err(TransferError::UndoRestoreFailed {
            path: path.to_path_buf(),
            reason: save
                .error
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".into()),
        });
    }
    Ok(())
}

/// Re-parses backup bytes and raises NEXTUNIQUEID to at least `floor`
/// so previously allocated IDs are never recycled.
fn bump_next_unique_id(bytes: &[u8], floor: u64, path: &Path) -> TransferResult<Vec<u8>> {
    let mut doc = parse_xml(bytes).map_err(|e| TransferError::UndoRestoreFailed {
        path: path.to_path_buf(),
        reason: format!("backup unparseable: {}", e),
    })?;
    let current = doc
        .root
        .get_attr("NEXTUNIQUEID")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if current < floor {
        doc.root.set_attr("NEXTUNIQUEID", floor.to_string());
    }
    Ok(serialize_xml(&doc))
}

// ─────────────────────────── Recovery (RD-M8-018~019) ───────────────────────────

/// What recovery did (or decided not to do) for one interrupted transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferRecoveryAction {
    /// Target was never committed; staged asset copies removed; source intact.
    RolledBack,
    /// Target committed and (for moves) source also committed; journal cleaned.
    Completed,
    /// Target committed but the source still contains the subtree. Recovery
    /// NEVER auto-deletes source data after a crash: the duplicate is kept
    /// and surfaced so the user can resolve it via
    /// [`complete_interrupted_move`]. (duplicate-not-loss bias)
    CompletedWithDuplicate,
    /// Journal unreadable; nothing touched; artifacts preserved for triage.
    Quarantined,
}

/// User-facing recovery report for one transaction (RD-M8-031).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecoveryReport {
    pub transaction_id: Option<String>,
    pub journal_path: PathBuf,
    pub operation: Option<TransferOperation>,
    pub action: TransferRecoveryAction,
    pub detail: String,
    /// Task IDs currently present in BOTH documents (duplicates to resolve).
    pub duplicate_task_ids: Vec<String>,
}

/// Scans the journal directory and recovers every interrupted transfer.
///
/// Guarantees (bias: duplicate-not-loss):
/// - Never deletes anything from a source document.
/// - Only removes staged asset copies when the target provably never
///   committed AND the staged file hash matches the journal's record.
/// - Corrupt journals are quarantined untouched.
pub fn recover_interrupted_transfers(journal_dir: &Path) -> Vec<TransferRecoveryReport> {
    let mut reports = Vec::new();
    for scanned in scan_journal_dir(journal_dir) {
        match scanned {
            ScannedJournal::Corrupt { path, reason } => {
                reports.push(TransferRecoveryReport {
                    transaction_id: None,
                    journal_path: path,
                    operation: None,
                    action: TransferRecoveryAction::Quarantined,
                    detail: format!("journal unreadable ({}); artifacts preserved", reason),
                    duplicate_task_ids: Vec::new(),
                });
            }
            ScannedJournal::Ok(journal) => {
                reports.push(recover_one(journal));
            }
        }
    }
    reports
}

fn recover_one(mut journal: TransferJournal) -> TransferRecoveryReport {
    let path = journal.path().to_path_buf();
    let manifest = journal.manifest().clone();
    let base = TransferRecoveryReport {
        transaction_id: Some(manifest.transaction_id.clone()),
        journal_path: path.clone(),
        operation: Some(manifest.operation),
        action: TransferRecoveryAction::RolledBack,
        detail: String::new(),
        duplicate_task_ids: Vec::new(),
    };

    // Stray completed journal: cleanup only.
    if journal.record().is_completed() {
        journal.remove();
        return TransferRecoveryReport {
            action: TransferRecoveryAction::Completed,
            detail: "transaction was already completed; journal cleaned".into(),
            ..base
        };
    }

    let target_has_subtree = document_contains_ids(
        &manifest.target_doc.path,
        &manifest.target_ids().into_iter().cloned().collect::<Vec<_>>(),
    );
    let source_has_subtree =
        document_contains_ids(&manifest.source_doc.path, &[manifest.root_task_id.clone()]);

    let journal_says_committed = journal.record().has_phase(JournalPhase::TargetCommitted);

    if journal_says_committed || target_has_subtree {
        // Target provably (or actually) holds the subtree.
        if manifest.operation == TransferOperation::Copy {
            // Copy: source was never supposed to change. Done.
            journal.append(JournalPhase::Completed, Some("finalized by recovery".into())).ok();
            journal.remove();
            return TransferRecoveryReport {
                action: TransferRecoveryAction::Completed,
                detail: "copy finished by recovery (target committed; source untouched)".into(),
                ..base
            };
        }
        // Move: check whether the source deletion already happened.
        if !source_has_subtree && journal.record().has_phase(JournalPhase::SourceCommitted) {
            journal.append(JournalPhase::Completed, Some("finalized by recovery".into())).ok();
            journal.remove();
            return TransferRecoveryReport {
                action: TransferRecoveryAction::Completed,
                detail: "move finished by recovery (both documents committed)".into(),
                ..base
            };
        }
        // Source still holds the subtree (or its state is unknown): keep the
        // duplicate, keep the journal as evidence, let the user resolve.
        let dups = if source_has_subtree && target_has_subtree {
            manifest
                .task_ids
                .iter()
                .filter_map(|s| manifest.mapped_id(s))
                .map(|t| t.as_str().to_string())
                .collect()
        } else {
            Vec::new()
        };
        return TransferRecoveryReport {
            action: TransferRecoveryAction::CompletedWithDuplicate,
            detail: "target committed but source subtree preserved (duplicate-not-loss); \
                     resolve via complete_interrupted_move or manual review"
                .into(),
            duplicate_task_ids: dups,
            ..base
        };
    }

    // Target never committed: roll back cleanly.
    // Remove staged asset copies (verified by hash) to avoid litter.
    let mut cleaned = 0usize;
    for asset in &manifest.asset_refs {
        if asset.classification != AssetClassification::CopyRequired {
            continue;
        }
        let (Some(dst), Some(expected)) = (&asset.target_abs, &asset.hash) else {
            continue;
        };
        if dst.is_file() {
            if let Ok(fp) = FileFingerprint::from_file(dst) {
                if &fp.hash == expected {
                    if std::fs::remove_file(dst).is_ok() {
                        cleaned += 1;
                    }
                }
            }
        }
    }
    journal.remove();
    TransferRecoveryReport {
        action: TransferRecoveryAction::RolledBack,
        detail: format!(
            "target never committed; source intact; {} staged asset copies removed",
            cleaned
        ),
        ..base
    }
}

/// True when the document at `path` parses and contains ALL given task IDs.
fn document_contains_ids(path: &Path, ids: &[TaskId]) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let Ok(doc) = parse_xml(&bytes) else {
        return false;
    };
    let mut all: Vec<String> = Vec::new();
    collect_task_ids_dfs(&doc.root, &mut all);
    let set: HashSet<&str> = all.iter().map(|s| s.as_str()).collect();
    ids.iter().all(|id| set.contains(id.as_str()))
}

/// Explicitly finishes an interrupted MOVE that recovery left as
/// `CompletedWithDuplicate`: revalidates the source fingerprint against the
/// journal, then performs the guarded source-delete phase and completes the
/// transaction. Fails safe (keeps the duplicate) on any mismatch.
pub fn complete_interrupted_move(
    journal_dir: &Path,
    transaction_id: &str,
) -> TransferResult<TransferRecoveryReport> {
    let journal_path = TransferJournal::path_for(journal_dir, transaction_id);
    let journal = TransferJournal::load(&journal_path)
        .map_err(|e| TransferError::RecoveryPrecondition {
            journal_path: journal_path.clone(),
            reason: e.to_string(),
        })?;
    if !journal.record().has_phase(JournalPhase::TargetCommitted) {
        return Err(TransferError::RecoveryPrecondition {
            journal_path,
            reason: "no durable TargetCommitted entry; refusing to delete source data".into(),
        });
    }
    if journal.manifest().operation != TransferOperation::Move {
        return Err(TransferError::RecoveryPrecondition {
            journal_path,
            reason: "transaction is not a move".into(),
        });
    }
    let mut journal = journal;
    // Rebuild a plan shell sufficient for the guarded source-delete phase.
    let plan = TransferPlan {
        manifest: journal.manifest().clone(),
        warnings: Vec::new(),
        subtree_xml: XmlElement::new("TASK"),
        new_target_next_unique_id: 0,
        target_root_task_count: 0,
        subtree_snapshot: rebuild_source_snapshot(&journal.manifest().source_doc.path, &journal.manifest().root_task_id)?,
    };
    commit_source_deletion(&mut journal, &plan)?;
    journal.append(JournalPhase::Completed, Some("completed by explicit resolution".into()))?;
    journal.remove();
    Ok(TransferRecoveryReport {
        transaction_id: Some(transaction_id.to_string()),
        journal_path: TransferJournal::path_for(journal_dir, transaction_id),
        operation: Some(TransferOperation::Move),
        action: TransferRecoveryAction::Completed,
        detail: "interrupted move completed: source subtree removed after fingerprint revalidation"
            .into(),
        duplicate_task_ids: Vec::new(),
    })
}

fn rebuild_source_snapshot(source_path: &Path, root_task_id: &TaskId) -> TransferResult<TaskTree> {
    let bytes = std::fs::read(source_path).map_err(|e| TransferError::SourceRead {
        path: source_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let doc = parse_xml(&bytes).map_err(|e| TransferError::SourceParse {
        path: source_path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let elem = find_task_element(&doc.root, root_task_id.as_str()).ok_or(
        TransferError::SourceTaskNotFound {
            path: source_path.to_path_buf(),
            task_id: root_task_id.as_str().to_string(),
        },
    )?;
    Ok(snapshot_subtree(elem))
}

// ─────────────────────────── UX backend (RD-M8-024~031) ───────────────────────────

/// One entry of the target-document picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTargetInfo {
    /// Stable workspace DocumentId.
    pub document_id: String,
    /// Display name (file name).
    pub display_name: String,
    /// Absolute path.
    pub path: PathBuf,
    /// Managed or linked.
    pub doc_type: String,
    /// False when the file is read-only / inaccessible for writing.
    pub writable: bool,
}

/// Enumerates writable workspace documents for the "复制到…/移动到…" picker,
/// excluding the source document.
pub fn list_transfer_targets(
    workspace: &Workspace,
    exclude_document_id: Option<&str>,
) -> Vec<TransferTargetInfo> {
    let mut out = Vec::new();
    for doc in workspace.documents() {
        if let Some(ex) = exclude_document_id {
            if doc.id == ex {
                continue;
            }
        }
        let path = workspace.resolve_document_path(doc);
        let writable = probe_writable(&path);
        out.push(TransferTargetInfo {
            document_id: doc.id.clone(),
            display_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            path,
            doc_type: doc.doc_type.as_str().to_string(),
            writable,
        });
    }
    out
}

fn probe_writable(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    if let Ok(md) = std::fs::metadata(path) {
        if md.permissions().readonly() {
            return false;
        }
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .is_ok()
}

// ─────────────────────────── Tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mtdl_transfer_{}_{}_{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_source(dir: &Path) -> PathBuf {
        let p = dir.join("source.xml");
        std::fs::write(
            &p,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<TODOLIST PROJECTNAME=\"Src\" NEXTUNIQUEID=\"10\" FILENAME=\"source.xml\">\r\n<TASK ID=\"1\" TITLE=\"Stay\" REFID=\"0\" COMMENTSTYPE=\"PLAIN_TEXT\" PRIORITY=\"5\" RISK=\"0\" PERCENTDONE=\"0\" POS=\"0\">\r\n</TASK>\r\n<TASK ID=\"5\" TITLE=\"Move Me\" REFID=\"0\" COMMENTSTYPE=\"PLAIN_TEXT\" PRIORITY=\"7\" RISK=\"0\" PERCENTDONE=\"0\" POS=\"1\" CUSTOMX=\"keepme\">\r\n    <TASK ID=\"6\" TITLE=\"Child\" REFID=\"0\" COMMENTSTYPE=\"PLAIN_TEXT\" PRIORITY=\"5\" RISK=\"0\" PERCENTDONE=\"0\" POS=\"0\">\r\n        <DEPENDENCY><TASKID>5</TASKID><DEPENDENCYTYPE>0</DEPENDENCYTYPE></DEPENDENCY>\r\n        <DEPENDENCY><TASKID>1</TASKID><DEPENDENCYTYPE>1</DEPENDENCYTYPE></DEPENDENCY>\r\n        <FILEREFPATH>.\\files\\note.txt</FILEREFPATH>\r\n        <FILEREFPATH>https://example.com/a</FILEREFPATH>\r\n        <COMMENTS>plain body</COMMENTS>\r\n    </TASK>\r\n</TASK>\r\n</TODOLIST>\r\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("files").join("note.txt"), b"asset-bytes").unwrap();
        p
    }

    fn write_target(dir: &Path) -> PathBuf {
        let p = dir.join("target.xml");
        std::fs::write(
            &p,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<TODOLIST PROJECTNAME=\"Tgt\" NEXTUNIQUEID=\"4\" FILENAME=\"target.xml\">\r\n<TASK ID=\"1\" TITLE=\"T1\" REFID=\"0\" COMMENTSTYPE=\"PLAIN_TEXT\" PRIORITY=\"5\" RISK=\"0\" PERCENTDONE=\"0\" POS=\"0\">\r\n</TASK>\r\n<TASK ID=\"2\" TITLE=\"T2\" REFID=\"0\" COMMENTSTYPE=\"PLAIN_TEXT\" PRIORITY=\"5\" RISK=\"0\" PERCENTDONE=\"0\" POS=\"1\">\r\n</TASK>\r\n</TODOLIST>\r\n",
        )
        .unwrap();
        p
    }

    fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf, TransferContext) {
        let dir = tmp(name);
        let src = write_source(&dir);
        // Target lives in its own subdirectory so document-relative asset
        // resolution has distinct source/target directories.
        let tgt_dir = dir.join("target");
        std::fs::create_dir_all(&tgt_dir).unwrap();
        let tgt = write_target(&tgt_dir);
        let ctx = TransferContext::for_workspace_root(&dir);
        (dir, src, tgt, ctx)
    }

    fn target_ids(path: &Path) -> Vec<String> {
        let bytes = std::fs::read(path).unwrap();
        let doc = parse_xml(&bytes).unwrap();
        let mut ids = Vec::new();
        collect_task_ids_dfs(&doc.root, &mut ids);
        ids
    }

    #[test]
    fn plan_allocates_in_target_id_space() {
        let (_d, src, tgt, _ctx) = setup("plan_ids");
        let plan = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions { operation: TransferOperation::Copy, ..Default::default() },
        )
        .unwrap();
        // Target has IDs 1,2 and NEXTUNIQUEID=4 → allocated 4,5.
        let mapped: Vec<String> = plan
            .manifest
            .id_map
            .iter()
            .map(|(_, t)| t.as_str().to_string())
            .collect();
        assert_eq!(mapped, vec!["4", "5"]);
        assert_eq!(plan.new_target_next_unique_id, 6);
        assert_eq!(plan.manifest.task_ids.len(), 2);
    }

    #[test]
    fn plan_corrects_low_next_unique_id() {
        let dir = tmp("plan_nuid");
        let src = write_source(&dir);
        let tgt = dir.join("target.xml");
        // NEXTUNIQUEID=2 but tasks 1,7 exist → effective next = 8.
        std::fs::write(
            &tgt,
            "<TODOLIST NEXTUNIQUEID=\"2\"><TASK ID=\"1\" TITLE=\"a\"></TASK><TASK ID=\"7\" TITLE=\"b\"></TASK></TODOLIST>",
        )
        .unwrap();
        let ctx = TransferContext::for_workspace_root(&dir);
        let plan = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions { operation: TransferOperation::Copy, ..Default::default() },
        )
        .unwrap();
        assert_eq!(plan.manifest.id_map[0].1.as_str(), "8");
        let _ = ctx;
    }

    #[test]
    fn plan_rewrites_internal_dep_and_marks_external() {
        let (_d, src, tgt, _ctx) = setup("plan_deps");
        let plan = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions { operation: TransferOperation::Copy, ..Default::default() },
        )
        .unwrap();
        // child (source 6 → target 5): dep on 5 (internal → 4), dep on 1 (external → preserved + FILENAME)
        let child = find_task_element(&plan.subtree_xml, "5").unwrap();
        let deps: Vec<&XmlElement> = child.children_by_tag("DEPENDENCY").collect();
        assert_eq!(deps.len(), 2);
        let d0_taskid = deps[0].first_child_by_tag("TASKID").unwrap().text_content();
        assert_eq!(d0_taskid, "4"); // internal remap
        assert!(deps[0].first_child_by_tag("FILENAME").is_none());
        let d1_taskid = deps[1].first_child_by_tag("TASKID").unwrap().text_content();
        assert_eq!(d1_taskid, "1"); // preserved
        assert_eq!(
            deps[1].first_child_by_tag("FILENAME").unwrap().text_content(),
            "source.xml"
        );
        assert!(plan.warnings.iter().any(|w| matches!(
            w,
            TransferWarning::ExternalDependencyPreserved { dep_task_id, .. } if dep_task_id == "1"
        )));
    }

    #[test]
    fn plan_block_policy_aborts() {
        let (_d, src, tgt, _ctx) = setup("plan_block");
        let err = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions {
                operation: TransferOperation::Copy,
                external_dependency_policy: ExternalDependencyPolicy::Block,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, TransferError::ExternalDependenciesBlocked { count: 1, .. }));
    }

    #[test]
    fn plan_classifies_assets() {
        let (_d, src, tgt, _ctx) = setup("plan_assets");
        let plan = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions { operation: TransferOperation::Copy, ..Default::default() },
        )
        .unwrap();
        let copy = plan
            .manifest
            .asset_refs
            .iter()
            .find(|a| a.classification == AssetClassification::CopyRequired)
            .unwrap();
        assert!(copy.original_ref.contains("note.txt"));
        let url = plan
            .manifest
            .asset_refs
            .iter()
            .find(|a| a.classification == AssetClassification::ReferenceOnly)
            .unwrap();
        assert!(url.original_ref.starts_with("https://"));
    }

    #[test]
    fn plan_rejects_same_document() {
        let (_d, src, _tgt, _ctx) = setup("plan_same");
        let err = plan_transfer(
            &src,
            &src,
            &TaskId::new("5"),
            &PlanOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, TransferError::SameDocument { .. }));
    }

    #[test]
    fn plan_missing_task() {
        let (_d, src, tgt, _ctx) = setup("plan_missing");
        let err = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("999"),
            &PlanOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, TransferError::SourceTaskNotFound { .. }));
    }

    #[test]
    fn copy_never_touches_source_and_commits_target() {
        let (_d, src, tgt, ctx) = setup("copy_ok");
        let src_before = std::fs::read(&src).unwrap();
        let out = copy_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |_| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), src_before);
        let ids = target_ids(&tgt);
        assert_eq!(ids, vec!["1", "2", "4", "5"]);
        assert_eq!(out.target_root_task_id.as_str(), "4");
        assert!(out.undo_available);
        // Asset copied next to target.
        assert!(tgt.parent().unwrap().join("files").join("note.txt").is_file());
        // Journal cleaned after completion.
        assert!(!TransferJournal::path_for(&ctx.journal_dir, &out.transaction_id).exists());
    }

    #[test]
    fn copy_preserves_unknown_attrs_and_comments_verbatim() {
        let (_d, src, tgt, ctx) = setup("copy_lossless");
        copy_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |_| {},
        )
        .unwrap();
        let bytes = std::fs::read(&tgt).unwrap();
        let doc = parse_xml(&bytes).unwrap();
        let moved = find_task_element(&doc.root, "4").unwrap();
        assert_eq!(moved.get_attr("CUSTOMX"), Some("keepme"));
        assert_eq!(moved.get_attr("TITLE"), Some("Move Me"));
        let child = find_task_element(moved, "5").unwrap();
        assert_eq!(
            child.first_child_by_tag("COMMENTS").unwrap().text_content(),
            "plain body"
        );
    }

    #[test]
    fn move_commits_target_then_source() {
        let (_d, src, tgt, ctx) = setup("move_ok");
        let out = move_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |_| {},
        )
        .unwrap();
        assert_eq!(target_ids(&tgt), vec!["1", "2", "4", "5"]);
        assert_eq!(target_ids(&src), vec!["1"]);
        assert!(out.source_fingerprint_after.is_some());
        assert!(!TransferJournal::path_for(&ctx.journal_dir, &out.transaction_id).exists());
    }

    #[test]
    fn move_undo_restores_both_documents() {
        let (_d, src, tgt, ctx) = setup("move_undo");
        let src_before = std::fs::read(&src).unwrap();
        let out = move_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |_| {},
        )
        .unwrap();
        undo_transfer(&ctx, &out.transaction_id).unwrap();
        assert_eq!(target_ids(&tgt), vec!["1", "2"]);
        assert_eq!(target_ids(&src), vec!["1", "5", "6"]);
        // Source restored byte-exact.
        assert_eq!(std::fs::read(&src).unwrap(), src_before);
        // Second undo rejected.
        assert!(matches!(
            undo_transfer(&ctx, &out.transaction_id),
            Err(TransferError::AlreadyUndone { .. })
        ));
    }

    #[test]
    fn copy_undo_removes_target_subtree_without_recycling_ids() {
        let (_d, src, tgt, ctx) = setup("copy_undo");
        let out = copy_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |_| {},
        )
        .unwrap();
        undo_transfer(&ctx, &out.transaction_id).unwrap();
        assert_eq!(target_ids(&tgt), vec!["1", "2"]);
        let bytes = std::fs::read(&tgt).unwrap();
        let doc = parse_xml(&bytes).unwrap();
        // NEXTUNIQUEID must not regress below the allocated IDs (no recycling).
        assert_eq!(doc.root.get_attr("NEXTUNIQUEID"), Some("6"));
    }

    #[test]
    fn move_detects_concurrent_source_edit_duplicate_not_loss() {
        let (_d, src, tgt, ctx) = setup("move_concurrent");
        let mut opts = PlanOptions::default();
        opts.operation = TransferOperation::Move;
        let plan = plan_transfer(&src, &tgt, &TaskId::new("5"), &opts).unwrap();
        // Simulate an external edit AFTER planning but BEFORE CommitSource.
        // We hook before BackupSource, mutate the source, then resume via
        // complete_interrupted_move which must refuse (fingerprint mismatch).
        let hooks = TransferHooks { fail_before: Some(TransferPhase::BackupSource) };
        let err = execute_transfer(plan, &ctx, &hooks, |_| {}).unwrap_err();
        assert!(matches!(err, TransferError::SimulatedKill { .. }));
        // External edit: append a comment byte-wise (changes fingerprint).
        let mut bytes = std::fs::read(&src).unwrap();
        bytes.extend_from_slice(b"<!-- external edit -->");
        std::fs::write(&src, &bytes).unwrap();

        let txn = {
            let scanned = scan_journal_dir(&ctx.journal_dir);
            match &scanned[0] {
                ScannedJournal::Ok(j) => j.manifest().transaction_id.clone(),
                _ => panic!("expected readable journal"),
            }
        };
        let reports = recover_interrupted_transfers(&ctx.journal_dir);
        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].action,
            TransferRecoveryAction::CompletedWithDuplicate
        );
        // Explicit resolution must refuse to delete the concurrently edited source.
        let err = complete_interrupted_move(&ctx.journal_dir, &txn).unwrap_err();
        assert!(matches!(err, TransferError::SourceConcurrentlyModified { .. }));
        // Data preserved in BOTH documents.
        assert!(!target_ids(&tgt).is_empty());
        assert!(target_ids(&tgt).contains(&"4".to_string()));
        let src_bytes = std::fs::read(&src).unwrap();
        assert!(String::from_utf8_lossy(&src_bytes).contains("Move Me"));
    }

    #[test]
    fn list_targets_reports_writability() {
        let dir = tmp("targets");
        let mut ws = Workspace::create(&dir, "WS".into()).unwrap();
        std::fs::write(dir.join("a.xml"), b"<TODOLIST NEXTUNIQUEID=\"1\"></TODOLIST>").unwrap();
        std::fs::write(dir.join("b.xml"), b"<TODOLIST NEXTUNIQUEID=\"1\"></TODOLIST>").unwrap();
        let a_id = ws.register_document("a.xml".into(), super::super::workspace::DocumentType::Managed).unwrap();
        ws.register_document("b.xml".into(), super::super::workspace::DocumentType::Managed).unwrap();
        // Make b.xml read-only.
        let mut perms = std::fs::metadata(dir.join("b.xml")).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(dir.join("b.xml"), perms).unwrap();

        let targets = list_transfer_targets(&ws, Some(&a_id));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].display_name, "b.xml");
        assert!(!targets[0].writable);

        // Restore perms for cleanup.
        let mut perms = std::fs::metadata(dir.join("b.xml")).unwrap().permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(dir.join("b.xml"), perms).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn progress_events_emitted() {
        let (_d, src, tgt, ctx) = setup("progress");
        let mut phases = Vec::new();
        move_task(
            &src,
            &tgt,
            &TaskId::new("5"),
            &PlanOptions::default(),
            &ctx,
            |p| phases.push(p),
        )
        .unwrap();
        assert!(phases.contains(&TransferProgress::CommittingTarget));
        assert!(phases.contains(&TransferProgress::BackingUpSource));
        assert!(phases.contains(&TransferProgress::CommittingSource));
        assert!(phases.contains(&TransferProgress::Finishing));
    }

    #[test]
    fn html_comment_relative_ref_planned_for_copy() {
        let dir = tmp("html_assets");
        let src = dir.join("source.xml");
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img").join("pic.png"), b"png-bytes").unwrap();
        std::fs::write(
            &src,
            "<TODOLIST NEXTUNIQUEID=\"3\"><TASK ID=\"1\" TITLE=\"h\" COMMENTSTYPE=\"HTML\"><COMMENTS>&lt;p&gt;&lt;img src=\"img/pic.png\"/&gt;&lt;/p&gt;</COMMENTS></TASK></TODOLIST>",
        )
        .unwrap();
        let tgt_dir = dir.join("target");
        std::fs::create_dir_all(&tgt_dir).unwrap();
        let tgt = write_target(&tgt_dir);
        let plan = plan_transfer(
            &src,
            &tgt,
            &TaskId::new("1"),
            &PlanOptions { operation: TransferOperation::Copy, ..Default::default() },
        )
        .unwrap();
        assert!(plan
            .manifest
            .asset_refs
            .iter()
            .any(|a| a.original_ref == "img/pic.png"
                && a.classification == AssetClassification::CopyRequired));
    }
}
