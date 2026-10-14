# ModernToDoList 2.0 Full Delivery Plan (M6 → M10)

## Summary

Total scope: ~90 Linear issues (INH-1042 ~ INH-1133), covering 185+ atomic RD/QA tasks across 6 phases. Execution follows the Critical Path: Stabilize → M6 → M7 → M8 → M9 → M10 → GA.

Each phase ends with: all tests passing → git commit → Linear status sync (comments with implementation details + test report) → GATE verification.

---

## Phase 0: Stabilization and Baseline (Pre-requisite)

**Goal**: Fix existing M2-M5 implementation gaps, commit all working-tree code, establish clean git baseline.

### 0.1 Fix Critical Implementation Gaps

| Gap | File(s) | Fix |
|-----|---------|-----|
| Empty command execute handlers | `src/app/commands.ts` | Wire `file.save` → `saveDocumentSession`, `edit.undo/redo` → session IPC, `task.addRoot` → allocate_task_id + AddTaskCommand, `workspace.open` → dialog + open_workspace |
| Inspector editors disconnected | `src/components/inspector/Inspector.vue` | Implement `@update:*` handlers calling new `update_task_field` IPC command |
| Missing `update_task_field` IPC | `src-tauri/src/commands/session.rs` + `src/ipc/client.ts` | Add command that invokes FieldUpdateCommand through UndoRedoManager |
| TaskRow checkbox disabled | `src/components/task-tree/TaskRow.vue` | Enable checkbox, wire to `update_task_field(status)` |
| Simplified session serialization | `src-tauri/src/commands/session.rs` `serialize_task_tree` | Use `domain::mappers::write_task` to rebuild XmlDocument preserving root attrs/unknown elements/comments |
| Child-count badge wrong value | `src/components/task-tree/TaskRow.vue` L115 | Show `childrenMap.get(task_key)?.length ?? 0` |
| Resize handles non-functional | `src/components/layout/MainLayout.vue` | Implement mousedown/mousemove/mouseup drag with min/max-width from tokens.css |

### 0.2 Git Hygiene

1. Ensure `.gitignore` excludes `node_modules/`, `dist/`, `src-tauri/target/`, `Data/`
2. `git add` all business source files (src/, src-tauri/src/, tests/, scripts/)
3. Remove `node_modules` from git tracking: `git rm -r --cached node_modules/`
4. Commit as "feat(m2-m5): complete IPC integration, workspace, task query and UI scaffold"
5. Merge `feat/m2-m3-ipc-integration` into `main`
6. Push to origin

### 0.3 Verify Baseline

- `cargo test` passes (round_trip + m4_qa + m5_qa = 49 tests)
- `npm run build` succeeds (Vite + TypeScript)
- `cargo tauri build` produces working executable

### 0.4 Linear Sync

- Fix INH-1133 (GATE-M10) status: Done → Backlog (incorrect state)
- Add stabilization comment to project

---

## Phase 1: M6 — Task Relations (19 issues: INH-1042 ~ INH-1060)

**Scope**: Participants, Dependencies, Progress Links, Attachments

### 1.1 Participants (INH-1042 ~ INH-1044)

**RD-M6-001~003** (INH-1042, P1):
- Domain: Define `ParticipantRef { display_name }` in `src-tauri/src/domain/participant.rs`
- Extend `Task` struct with `participants: Vec<ParticipantRef>` (separate from existing `allocated_to`)
- XML mapping: Parse/write via AllocatedTo native mapping in `mappers.rs`, preserve source ordering
- Indexer: Populate `task_participants` table (schema already exists in `schema.rs`)
- IPC: Add `get_participants`, `add_participant`, `remove_participant` commands

**RD-M6-004~006** (INH-1043, P2):
- Frontend: `src/components/inspector/ParticipantsEditor.vue` — chips with remove + add picker
- Suggestion: Query distinct participant names from workspace index
- Bulk edit: Multi-select participant assignment

**RD-M6-007~008** (INH-1044, P2):
- Extend `filter-state.ts` with participant filter condition
- Add participant grouping view mode in TaskTree

### 1.2 Dependencies (INH-1045 ~ INH-1047)

**RD-M6-009~012** (INH-1045, P1):
- Domain: `src-tauri/src/domain/dependency.rs` — `TaskRef::{Local, External, Unresolved}`
- Parse DEPENDENCY elements from XML via mappers (partial support exists in task.rs)
- External refs: Cross-document using DocumentId+TaskId

**RD-M6-013~016** (INH-1046, P1):
- Commands: `add_dependency`, `remove_dependency` with cycle detection (DFS/BFS)
- Reject self-reference; validate source/target existence
- Maintain `task_dependencies` index with forward + reverse lookups
- UndoableCommand integration

**RD-M6-017~022** (INH-1047, P2):
- Frontend: `src/components/inspector/DependencyEditor.vue` — task picker/search
- Show outgoing "依赖" and incoming "阻塞了" separately
- Blocked indicator on TaskRow; degraded states for unresolved/circular refs

### 1.3 Progress Links (INH-1048 ~ INH-1049)

**RD-M6-023~026** (INH-1048, P1):
- Domain: `src-tauri/src/domain/progress_link.rs` — `ProgressLink { id, label, url, provider }`
- Provider detection: GitHub/Linear/Jira/generic from URL pattern
- URL validation: Allow http/https only; reject javascript:/data: schemes
- XML storage: Use Tier B custom attribute/extension mechanism per M0 compatibility audit
- Index: Populate `progress_links_index` table

**RD-M6-027~030** (INH-1049, P2):
- Frontend: `src/components/inspector/ProgressLinksEditor.vue` — provider icon + label + URL
- Open action via Windows default browser (tauri_plugin_shell)
- Add/edit/remove with UndoableCommand

### 1.4 Attachments (INH-1050 ~ INH-1054)

**RD-M6-031~032,036~038** (INH-1050, P1):
- Domain: `src-tauri/src/domain/attachment.rs` — `AttachmentKind::{ManagedFile, LinkedFile, Url}`
- `AttachmentRef { id, kind, display_name, path_or_url, size, hash }`
- Asset root policy: `.assets/<document-id>/attachments/` per-document managed directory
- Path safety: No traversal outside asset root; collision-safe naming (UUID + extension)

**RD-M6-033** (INH-1051, P1):
- Transactional import: Validate source → copy to staging → verify hash → atomic rename to final → update XML reference → update index
- Rollback on any failure; source file never modified

**RD-M6-034~035,039~040** (INH-1052, P1):
- Linked file: Store validated path reference only (no copy)
- URL attachment: Validate scheme, store native FileLink representation
- Rebuildable `attachments_index` from XML scan

**RD-M6-041~045** (INH-1053, P2):
- Frontend: `src/components/inspector/AttachmentListEditor.vue`
- Type icons, display name, status indicators
- Actions: Add Managed / Link Local / Add URL / Open / Reveal in Explorer
- Drag-and-drop import; missing-file state display

**RD-M6-046~048** (INH-1054, P1):
- Removal semantics: Linked/URL → delete reference only; Managed → mark orphan candidate
- Orphan tracking: `.assets/<doc>/orphans.manifest`
- BLAKE3 hash metadata for integrity verification

### 1.5 M6 QA and Gate

**QA Issues** (INH-1055 ~ INH-1059):
- QA-M6-001~002: Participant round-trip + unknown-data preservation
- QA-M6-003~008: Dependency compatibility, graph-safety, index-rebuild
- QA-M6-009~010: Progress Link persistence + URL security
- QA-M6-011~017: Attachment lifecycle, relocation, integrity
- QA-M6-018: M0-M5 cumulative regression with M6 features

**GATE-M6** (INH-1060): All checks pass → mark Done → Linear sync

### 1.6 M6 Execution Order (internal dependencies)

```
Participants domain (001~003) ─┬─→ Participant UI (004~006) ─→ Filter/Group (007~008)
                               │
Dependency domain (009~012) ───┼─→ Dependency mutations (013~016) ─→ Dependency UI (017~022)
                               │
Progress Link domain (023~026)─┼─→ Progress Link UI (027~030)
                               │
Attachment domain (031~038) ───┴─→ Import (033) ─→ Index (034~040) ─→ UI (041~045) ─→ Removal (046~048)
                                                                                         │
                                                                            QA-M6-001~018 ─→ GATE-M6
```

Parallelizable: Participants, Dependencies, Progress Links, Attachments domain layers can be developed concurrently.

---

## Phase 2: M7 — Rich Content (14 issues: INH-1061 ~ INH-1074)

**Scope**: Tiptap editor integration, HTML sanitizer, image assets, comment type preservation

### 2.1 Editor Foundation (INH-1061 ~ INH-1062)

**RD-M7-001~002** (INH-1061, P1):
- Add Tiptap OSS packages: `@tiptap/vue-3`, `@tiptap/starter-kit`, `@tiptap/extension-*` (link, image, underline, strike, text-align, color, highlight, code-block-lowlight)
- Build `src/components/editor/RichTextEditor.vue` with edit/preview modes
- Feed content from TaskDescription type; separate safe preview rendering path
- Never auto-create or auto-convert Comments type on view

**RD-M7-003~010** (INH-1062, P2):
- Toolbar component: Bold, Italic, Underline, Strike, Headings (H1-H3), Font Size (limited), Text Color, Bullet/Ordered List, Blockquote, Link, Inline Code, Code Block
- Keyboard shortcuts mapped to editor commands
- Toolbar state reflects current selection

### 2.2 Security (INH-1063)

**RD-M7-011~013** (INH-1063, P1):
- HTML whitelist sanitizer (Rust-side using `scraper` or `ammonia` crate, or JS-side DOMPurify)
- Allowed tags: p, h1-h6, strong, em, u, s, span, ul, ol, li, blockquote, a, code, pre, img, br
- Allowed attributes: href (http/https only), src (relative managed paths), class, style (limited)
- Strip: script, iframe, object, embed, event handlers (on*), javascript: URLs, data: URIs
- Apply on: paste import, Core→Editor feed, Editor→Core commit, preview render

### 2.3 Comment Type Preservation (INH-1064)

**RD-M7-014~018** (INH-1064, P1):
- Plain Text mode: plain-text editor (textarea/contenteditable); save keeps COMMENTSTYPE unchanged
- HTML mode: Tiptap editor; save produces sanitized HTML
- Explicit conversion action: "Convert to Rich Text" with confirmation dialog; reversible via Undo
- RTF/Unknown types: Read-only display with type indicator; no edit/convert allowed
- CommentsType enum in domain layer drives editor mode selection

### 2.4 Image Assets (INH-1065 ~ INH-1066)

**RD-M7-019~023** (INH-1065, P1):
- Clipboard paste handler: capture image bytes → send to Rust Asset Service
- Drag-drop handler: file drag → managed import
- Storage: `.assets/<doc>/images/<uuid>.<ext>` with BLAKE3 verification
- Rich-text reference: relative path `<img src="../.assets/doc-id/images/uuid.png">`
- No Base64 in XML by default

**RD-M7-024~025** (INH-1066, P1):
- Orphan tracking: image removed from rich text → mark as orphan candidate
- GC eligibility: no live reference in XML/rich-text/Undo-history/Recovery-journal
- Explicit GC command (user-triggered or workspace maintenance)

### 2.5 Undo and Autosave (INH-1067)

**RD-M7-026~027** (INH-1067, P1):
- Editor-local history (Tiptap built-in) for granular text undo
- Coalesced semantic commits to Core: debounce 1-2s after last keystroke
- Core-level Undo: one description field update = one UndoableCommand
- Autosave: respect existing M3 session autosave interval; never serialize per keystroke

### 2.6 Search Text Extraction (INH-1068)

**RD-M7-028** (INH-1068, P2):
- `extract_plain_text(comments: &TaskComments) -> String`
- Plain Text: return as-is
- HTML: sanitize → strip tags → sensible block separators
- RTF/Unknown: return empty or metadata placeholder

### 2.7 M7 QA and Gate

- QA-M7-001~006: Comments type preservation matrix
- QA-M7-007~009: XSS and dangerous URL sanitation
- QA-M7-010~015: Managed image asset lifecycle
- QA-M7-016: Large rich-text performance (no per-keystroke serialization)
- QA-M7-017: M0-M6 cumulative regression
- GATE-M7 (INH-1074)

---

## Phase 3: M8 — Multi-Document (17 issues: INH-1075 ~ INH-1094)

**Scope**: Cross-document transfer transactions, File Library, Trash

### 3.1 Transfer Transaction Core (INH-1077 ~ INH-1081)

**RD-M8-001~005** (INH-1077, P1):
- `TransferManifest { transaction_id, operation, source_doc, target_doc, task_ids, id_map, asset_refs }`
- Source subtree snapshot: deep clone task tree with all relations
- Target ID map: allocate new TaskIds in target document's ID space
- Logical clone: rewrite internal references using ID map

**RD-M8-006~010** (INH-1079, P1):
- Internal dependencies (both tasks in subtree): rewrite to new target IDs
- External dependencies (pointing outside): documented policy (preserve as External ref / warn / block)
- Attachment/asset plan: classify each as copy-required / reference-only / skip
- Progress links: preserve URLs, remap document-relative paths

**RD-M8-011~014** (INH-1080, P1):
- Target-first commit: stage assets → write target XML temp → validate → atomic replace
- Never delete source until target is proven committed
- Transfer Journal: record each phase transition for crash recovery

**RD-M8-015~019** (INH-1081, P1):
- Source deletion phase: only after verified `target_committed` journal entry
- Revalidate source fingerprint before deletion (detect concurrent external edits)
- Recovery: interrupted transfers leave duplicate-not-loss (bias toward data preservation)
- Journal cleanup after successful completion

### 3.2 Transfer Services and UX (INH-1082 ~ INH-1083)

**RD-M8-020~023** (INH-1082, P1):
- `copy_task`: planning + target-first pipeline; never deletes source
- `move_task`: target-first + source-delete phases
- Transaction-level Undo: completed Copy is reversible (delete target subtree); completed Move is reversible (move back)
- Structured error types for each failure mode

**RD-M8-024~031** (INH-1083, P1):
- Context menu: "复制到…" / "移动到…" for selected task/subtree
- Target document picker: list writable workspace documents
- Progress indicator during transfer
- Recovery results display after interrupted transfers

### 3.3 File Library (INH-1084 ~ INH-1088)

**RD-M8-032** (INH-1084, P2): New Managed Document creation in Workspace
**RD-M8-033~035** (INH-1085, P1): Document rename, move, duplicate with stable DocumentId
**RD-M8-036~038** (INH-1086, P1): Import as Managed / Link External / Remove Reference
**RD-M8-039~042** (INH-1087, P1): Trash manifest under `.moderntodo/trash/`, restore, permanent delete
**RD-M8-043~045** (INH-1088, P1): Reveal in Explorer, Undo policy, path-reference repair

### 3.4 M8 QA and Gate

- QA-M8-001~006: Transfer first-half crash matrix (kill before/during target commit)
- QA-M8-007~012: Source-commit, journal-corruption, I/O failure matrix
- QA-M8-013~016: Dependency and attachment remap verification
- QA-M8-017~019: File Library Trash/Linked/rename regression
- QA-M8-020: M0-M7 full cumulative regression
- GATE-M8 (INH-1094): P0 data safety gate — any QA failure blocks

---

## Phase 4: M9 — Productivity (15 issues: INH-1095 ~ INH-1109)

**Scope**: Global Search (FTS5), Smart Views, Saved Views, Command Palette, Quick Add

### 4.1 Search Infrastructure (INH-1095 ~ INH-1097)

**RD-M9-001~003** (INH-1095, P1):
- FTS5 migration: `task_search_fts` table with UNINDEXED identity columns (document_id, task_id)
- Index: task Title + description plain text (from M7 extractor)
- Rebuildable from XML scan; not a business source

**RD-M9-004~007** (INH-1096, P2):
- Expand searchable text: tags, participant names, attachment display names, progress link labels
- Join strategy: FTS5 content table or parallel indexed columns

**RD-M9-008~013** (INH-1097, P1):
- SearchResult DTO: TaskKey, title, document context, matched-field snippet, score
- Application service: bounded results, deterministic ordering, pagination
- Chinese fallback: FTS5 unicode61 tokenizer limitations → LIKE-based substring fallback for CJK
- Frontend: `src/components/search/GlobalSearch.vue` — input, results list, keyboard nav, task jump
- Incremental index updates on task mutations

### 4.2 Smart Views (INH-1098 ~ INH-1099)

**RD-M9-014~019** (INH-1098, P1):
- Today: unfinished tasks with due_date = today OR start_date <= today
- Upcoming: grouped by date (Today/Tomorrow/This Week/Later)
- Overdue: due_date < today AND status != Completed/Cancelled
- Unscheduled: no start_date AND no due_date
- Completed: status = Completed, sorted by completed_date desc
- Flagged: conditional on Priority >= High or custom flag

**RD-M9-020~021** (INH-1099, P2):
- By Participant: group tasks by participant name
- By Tag: group tasks by tag/category

### 4.3 Saved Views (INH-1100 ~ INH-1101)

**RD-M9-022~027** (INH-1100, P2):
- Structured predicate model: `ViewPredicate { field, operator, value }` with AND/OR composition
- Versioned serialization to `saved_views` SQLite table (schema exists)
- Evaluator: compile predicates → SQL WHERE or in-memory filter

**RD-M9-028~030** (INH-1101, P2):
- CRUD operations: create from current filter state, rename, delete
- Sidebar rendering: list saved views with icon/count
- Persistence: survives restart; disposable application state (not business data)

### 4.4 Command Palette and Quick Add (INH-1102 ~ INH-1104)

**RD-M9-031~034** (INH-1102, P2):
- Finalize Command Registry: all app commands with id, label, icon, shortcut, category
- Ctrl+K palette: fuzzy search over commands + task titles
- Task jump: select result → navigate to task in tree
- Keyboard-first navigation

**RD-M9-035~041** (INH-1103, P2):
- Quick Add grammar: `Task title #tag @participant !priority due:2024-01-15`
- Parser: tokenize → extract title, tags, participants, priority, dates
- Safe fallback: unrecognized tokens remain part of title
- Target: current document + selected parent (or root)

**RD-M9-042~043** (INH-1104, P2):
- Optional global shortcut (registered via Tauri global shortcut API)
- Conflict detection: if OS/other app holds the shortcut, degrade gracefully
- Settings toggle for enable/disable

### 4.5 M9 QA and Gate

- QA-M9-001~005: Global Search correctness, Chinese fallback, rebuild
- QA-M9-006~010: Smart/Saved View date boundaries, non-mutating persistence
- QA-M9-011~015: Quick Add, command conflict, jump, canonical-filter regression
- QA-M9-016: M0-M8 cumulative regression
- GATE-M9 (INH-1109): Feature Complete — enables M10 RC scope freeze

---

## Phase 5: M10 — Performance, Portable Hardening and GA (25 issues: INH-1110 ~ INH-1133)

**Scope**: Benchmarks, profiling, virtualization decision, migration, portable hardening, release

### 5.1 Benchmark Infrastructure (INH-1110 ~ INH-1112)

**RD-M10-001** (INH-1110, P2): Deterministic fixture generator
- Seeded PRNG → generate XML task trees with configurable depth/breadth/field density
- Output: reproducible fixture files with SHA-256 manifest

**RD-M10-002~009** (INH-1111, P2): Canonical performance datasets
- Scale variants: 1k / 10k / 50k / 100k tasks
- Shape variants: deep-tree (depth 20+), rich-text-heavy, attachment-heavy, dependency-heavy
- All generated deterministically from fixture generator

**RD-M10-010** (INH-1112, P2): Versioned benchmark result format
- JSON schema: `{ version, timestamp, dataset, metrics: { name, value, unit, percentile } }`
- Human-readable Markdown report template

### 5.2 Profiling (INH-1113 ~ INH-1114)

**RD-M10-011~015** (INH-1113, P2): Core profiling
- Startup time (cold/warm), workspace scan, XML parse throughput, index rebuild, search latency
- Instrument with `std::time::Instant` or `tracing` spans

**RD-M10-016~021** (INH-1114, P2): UI/memory profiling
- Tree render FPS, inspector edit latency, autosave overhead, cross-file move, peak memory, WebView2 cache growth
- Frontend: Performance API marks; Backend: process memory sampling

### 5.3 Virtualization Decision (INH-1115 ~ INH-1116)

**RD-M10-022** (INH-1115, P1): Decision gate
- Evaluate profiling evidence: if tree render > 16ms frame budget at 10k+ visible rows → IMPLEMENT
- If under budget → SKIP (document rationale)
- Decision recorded in ADR format

**RD-M10-023** (INH-1116, P2): Conditional implementation
- Virtual scroll for TaskTree: only render visible rows + overscan buffer
- Implement ONLY if RD-M10-022 gate says IMPLEMENT
- Use `vue-virtual-scroller` or custom implementation with fixed/variable row heights

### 5.4 Portable Hardening (INH-1117 ~ INH-1123)

**RD-M10-024** (INH-1117, P2): Deterministic release ZIP
- Reproducible build: fixed timestamps, sorted entries, consistent compression
- Output: `ModernToDoList-2.0.0-Portable-win-x64.zip` with SHA-256 hash

**RD-M10-025~027** (INH-1118, P1): Versioned Data/settings/index migration
- Migration framework: detect Data directory version → apply sequential migrations
- Settings migration: schema version in `Data/settings.json`
- Index rebuild fallback: if migration fails → delete index.db → full rebuild from XML

**RD-M10-028~029** (INH-1119, P2): WebView2 diagnostics + offline package
- Runtime detection with structured error reporting
- Offline fixed-version package policy documentation
- Fallback: user-facing download instructions

**RD-M10-030~033** (INH-1121, P2): Path hardening
- Removable drive: detect and handle drive letter changes
- Chinese/space characters in paths: full Unicode path testing
- Long paths (>260 chars): `\\?\` prefix on Windows
- Read-only EXE directory: Data/ always writable relative to user profile if EXE dir is read-only

**RD-M10-034** (INH-1120, P2): Antivirus false-positive investigation
- Document common AV triggers in Tauri/Electron apps
- Establish investigation process and mitigation checklist
- Code signing policy documentation

**RD-M10-035~036** (INH-1122, P2): Crash logs + diagnostics
- Structured crash dump: timestamp, stack trace, last operations, system info
- Privacy-safe: no task content, no file paths (hash only)
- Export action for user-submitted bug reports

**RD-M10-037~040** (INH-1123, P2): Release finalization
- LICENSES/: Collect all dependency licenses (Rust crates + npm packages)
- Version metadata: embedded in EXE (file version, product version, company)
- Manual update path: document how to update Portable ZIP
- Release notes: template + 2.0.0 changelog

### 5.5 M10 QA Matrices (INH-1124 ~ INH-1131)

8 parallel regression matrices (all P1):
- **A** (XML Compatibility): 13 cases — all encoding/unknown/comment/dependency/filelink fixtures
- **B** (Atomic Save/Recovery): 10 cases — kill-before/mid-write, disk full, permission denied, external conflict
- **C** (Workspace/SQLite): 10 cases — managed/linked docs, index rebuild, corruption, UNC, watcher
- **D** (Core UX): 10 cases — CRUD, nested, reorder, filter, multi-select, keyboard, undo/redo, autosave
- **E** (Relations/Attachments): 12 cases — participants, dependencies, progress links, attachments, rich content
- **F** (Cross-document): 10 cases — copy/move subtree, dependency remap, crash recovery, transfer undo
- **G** (Productivity): 10 cases — search, Chinese search, smart views, saved views, quick add, command palette
- **H** (Windows Portable): 15 cases — clean VM, no network/admin/Node/Rust, move app, removable drive, Chinese path

### 5.6 RC Review and GA Gate

**RC-M10** (INH-1132, P1):
- Freeze commit/version/ZIP hash
- Classify all remaining issues as Blocker-Data/Persistence/Portable/Security/Core-UX
- Blocker count must = 0

**GATE-M10** (INH-1133, P1):
- All Blockers = 0; all P0/P1 data safety issues = 0
- M0-M9 automated tests all pass + RC full matrix pass
- Verified: delete index.db → rebuild → full function
- Verified: cross-file Move kill recovery
- Verified: Win10 + Win11 clean VM + no-network Portable
- Release ZIP hash fixed; migration test pass; release notes complete

---

## Cross-Cutting Concerns

### Linear Sync Protocol

For each completed issue:
1. Change state to "In Progress" when starting work
2. Add comment with: implementation details (what/how/files), test report (what tested/results)
3. Change state to "Done" when verified
4. Use `scripts/linear-sync-comments.cjs` pattern (Node.js fetch, UTF-8 guaranteed)
5. GraphQL mutations MUST use `variables` format (never inline values)

### Git Commit Strategy

- One commit per logical RD task group (not per file)
- Conventional commits: `feat(m6): implement participant domain and XML mapping`
- Push after each milestone GATE passes
- Branch strategy: work on `main` directly (feature branches already merged in Phase 0)

### Encoding Safety (Anti-Mojibake)

- All Linear API calls via Node.js (`fetch` with explicit `Content-Type: application/json; charset=utf-8`)
- Never use PowerShell for API calls with Chinese content
- Script files saved as UTF-8 without BOM
- Comment bodies constructed in JS objects, JSON.stringify handles encoding

### Testing Strategy

- Rust unit tests: each new domain module gets `#[cfg(test)] mod tests`
- Integration tests: `src-tauri/tests/m6_qa_tests.rs`, `m7_qa_tests.rs`, etc.
- Frontend: Manual E2E via Browser agent for UI verification
- Cumulative regression: each milestone re-runs all prior test suites
- Fixture-based: extend `tests/fixtures/xml/` with new M6-M9 scenario fixtures

---

## Estimated Execution Scale

| Phase | RD Tasks | QA Tasks | New Rust Modules | New Vue Components | Est. Files Changed |
|-------|----------|----------|-----------------|-------------------|-------------------|
| 0 | 7 fixes | baseline | 0 | 0 | ~15 |
| M6 | 48 atomic | 18 | 4 (participant, dependency, progress_link, attachment) | 5 | ~30 |
| M7 | 28 atomic | 17 | 2 (sanitizer, rich_text) | 3 (editor, toolbar, preview) | ~25 |
| M8 | 45 atomic | 20 | 3 (transfer, file_library, trash) | 4 | ~30 |
| M9 | 43 atomic | 16 | 2 (search_fts, quick_add) | 5 (search, views, palette, quick-add) | ~25 |
| M10 | 40 atomic | 80+ | 2 (benchmark, migration_v2) | 1 (virtual-scroll, conditional) | ~20 |
| **Total** | **~211** | **~151** | **~13** | **~18** | **~145** |

---

## Risks and Mitigations

1. **M0 Compatibility Audit blocker**: Progress Links and HTML Comments depend on M0 Tier B audit conclusions. If the audit hasn't formally approved a storage mechanism, implement using custom XML attributes with `unknown_attrs` preservation as fallback.

2. **Scale**: 90 issues is multi-week work. Mitigation: parallelize independent domain modules within each milestone; batch Linear sync at milestone boundaries.

3. **API Key exposure**: Linear API key is hardcoded in tracked scripts. Mitigation: externalize to environment variable before any push; add `.env` to `.gitignore`.

4. **Unsafe Send impl**: `SessionEntry` uses `unsafe impl Send`. Mitigation: add `+ Send` bound to `UndoableCommand` trait during M6 work when touching session code.

5. **WebView2 dependency**: Clean VM testing requires WebView2 runtime. Mitigation: document offline installer bundling in RD-M10-028~029.
