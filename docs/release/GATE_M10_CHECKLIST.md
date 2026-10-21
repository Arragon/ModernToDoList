# GATE-M10 — ModernToDoList 2.0 GA Exit Gate — Audit Record

**Linear:** INH-1133 · **Milestone:** M10 · **Priority:** P1
**Audit date:** 2026-09-22, refreshed 2026-10-21 · **Repo:** `Arragon/ModernToDoList` · **Branch:** `main`
**Predecessor:** RC-M10 (INH-1132, Done 2026-10-21) → `RC_M10_BLOCKER_REVIEW.md`

## VERDICT: **NOT PASSED — gate remains open**

All product-category Blockers are now **0** (Data / Persistence / Portable / Security / Core-UX),
the 2.0.0 RC is frozen (commit `0dda444`, tag `v2.0.0-rc`, ZIP SHA-256
`a6c89880da9c9c7188edaf84199de6892f83e1223c19c0a00e4ad00ad66aa58e`), and every automated criterion
below is MET. The single remaining gap is **matrix H**: it requires clean Win10 + Win11 VMs with no
network and no admin rights, which cannot be executed on this host (no Hyper-V feature enabled, no
ISOs — see `docs/qa/LEFTOVER_TASKS.md` L-04). Per the delivery plan and §8 below, this gate must not
be closed on the strength of passing unit tests alone, so it stays open until a human executes H01–H15
against the frozen RC ZIP.

Status values: `MET` · `NOT MET` · `PARTIAL` · `CANNOT VERIFY HERE`.

---

## 1. Blocker and data-safety criteria

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1.1 | All Blockers = 0 | **MET (product) / PARTIAL (process)** | `RC_M10_BLOCKER_REVIEW.md` regenerated 2026-10-21 from live Linear state: **product categories all 0** (Data 0, Persistence 0, Portable 0, Security 0, Core-UX 0). 4 open items remain, all process gates: INH-718 (M10 epic), INH-1131 (H matrix, environment-blocked), INH-1132 (RC review — since closed Done), INH-1133 (this gate). Reduced from 94 at Phase 0 |
| 1.2 | All P0/P1 data-safety issues = 0 | **MET** | All Data-classified blockers resolved. The last data-safety defect found (session open of workspace-relative paths, §7 fifth defect) was fixed at `a586175` with regression tests before the freeze |
| 1.3 | No issue closed while its dependencies are open | **MET** | Verified by direct query. Phase 0 found INH-1133 marked `Done` with all 93 M6–M10 issues in Backlog; corrected to Backlog. Post-sync re-check: all 5 gates (INH-1060/1074/1094/1109/1133) are **not** Done while their dependencies remain open |

## 2. Automated test criteria

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 2.1 | M0–M9 automated tests all pass | **MET** | Final run on the frozen commit `0dda444`: **1260 passed, 0 failed** (2 ignored: the full-scale benchmark generator and the release-ZIP generator), 20 targets. Full output archived at `docs/qa/evidence/m10-final-test-run.txt` |
| 2.2 | Frontend type-checks and builds | **MET** | `vue-tsc --noEmit && vite build` on 2026-10-21: **0 TypeScript errors**, dist rebuilt for the 2.0.0 freeze |
| 2.3 | XML round-trip compatibility intact | **MET** | `round_trip_test` 17/17 (encodings, unknown elements/attributes, comments, dependencies, FileLink, real-world fixture) plus matrix A 14/14 |
| 2.4 | Index remains disposable | **MET** | `qa_m10_c09_gate_delete_index_db_rebuild_full_function` and `qa_m9_016` both delete `index.db`, rebuild from XML and assert full function; re-verified live 2026-10-21 (delete → restart → rebuild → full function) |

## 3. RC full matrix (QA-M10 A–H)

| Matrix | Cases | Linear | Status | Evidence |
|--------|-------|--------|--------|----------|
| A — XML compatibility | A01~A13 | INH-1124 | **MET** | `m10_qa_a_xml_compat.rs` 14/14. Surfaced and fixed a real data-loss bug (§6) |
| B — Atomic save / recovery | B01~B10 | INH-1125 | **MET** | `m10_qa_b_atomic_save.rs` 10/10. Permission-denied provoked for real via a read-only decoy at the deterministic `.doc.xml.tmp` path |
| C — Workspace / SQLite | C01~C10 | INH-1126 | **MET** | `m10_qa_c_workspace_sqlite.rs` 10/10, stable over 4 repeat runs |
| D — Core task UX | D01~D10 | INH-1127 | **MET** | `m10_qa_d_core_ux.rs` 10/10, asserting canonical TaskId stability |
| E — Relations / attachments / rich content | E01~E12 | INH-1128 | **MET** | `m10_qa_e_relations_richtext.rs` 12/12, asserting cross-cutting invariants (relations survive a save AND stay searchable AND round-trip losslessly) |
| F — Cross-document transactions | F01~F10 | INH-1129 | **MET** | `m10_qa_f_crossdoc.rs` 10/10; every crash point asserts the union-no-loss invariant |
| G — Productivity | G01~G10 | INH-1130 | **MET** | `m10_qa_g_productivity.rs` 10/10, including a real CJK case where plain FTS5 returns nothing and the LIKE fallback hits |
| H — Windows Portable / clean machine | H01~H15 | INH-1131 | **CANNOT VERIFY HERE** | Requires clean Win10 + Win11 VMs, no network, no admin rights, no Node/Rust/Python/Git. Protocol delivered at `docs/qa/QA_M10_H_PORTABLE_PROTOCOL.md`; **0 of 15 cases executed** |

## 4. Explicit verification items (spec §5.6)

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 4.1 | Delete `index.db` → rebuild → full function | **MET** | Automated: `qa_m10_c09`, `qa_m10_c10`, `qa_m9_016` |
| 4.2 | Cross-file Move kill recovery | **MET** | `m8_qa_tests` asserts no-task-loss at every crash point and `m10_qa_f_crossdoc` asserts the union-no-loss invariant under the F-matrix numbering — both green in the frozen-commit run. CDP E2E E4/E5 additionally drove the Recovery Required and completed_with_duplicate paths through the real UI (evidence at `bcddfde`) |
| 4.3 | Win10 + Win11 clean VM | **CANNOT VERIFY HERE** | Matrix H — no VMs available |
| 4.4 | No-network Portable run | **CANNOT VERIFY HERE** | Matrix H case H03 |

## 5. Release artifact criteria

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 5.1 | Release ZIP hash fixed | **MET** | Deterministic ZIP (`scripts/release/make-portable-zip.cjs`, sorted entries + fixed DOS timestamps) built **twice** from `release/staging-2.0.0` on 2026-10-21, byte-identical. Frozen SHA-256: `a6c89880da9c9c7188edaf84199de6892f83e1223c19c0a00e4ad00ad66aa58e` for `ModernToDoList-2.0.0-Portable-win-x64.zip` (4,107,682 bytes, 6 entries) |
| 5.2 | Migration test passes | **MET** | `infrastructure/migration.rs` v0→3 Data migration, settings `schemaVersion` 0→2, and the failure→`rebuild_index_fallback` path; 14 lib tests |
| 5.3 | Release notes complete | **MET** | `docs/release/RELEASE_NOTES_2.0.0.md` — date corrected to 2026-10-21, frozen ZIP SHA-256 recorded in the header, publishing checklist ticked |
| 5.4 | Licenses collected | **MET** | `LICENSES/RUST_DEPENDENCIES.md` (566 crates from `Cargo.lock`), `NPM_DEPENDENCIES.md` (152 packages from `package-lock.json`), `THIRD_PARTY_NOTICES.md`; shipped inside the ZIP under `LICENSES/` |
| 5.5 | Version metadata embedded | **MET** | `tauri.conf.json` / `package.json` / `package-lock.json` / `Cargo.toml` / `Cargo.lock` all set to **2.0.0** at the freeze commit `0dda444` (the 9-22 gap is closed) |
| 5.6 | Manual update path documented | **MET (doc only)** | `docs/release/MANUAL_UPDATE.md`. Validation is matrix H case H15 → **CANNOT VERIFY HERE** |
| 5.7 | WebView2 policy documented | **MET (doc only)** | `docs/release/WEBVIEW2_POLICY.md` + structured detection in `platform/windows/webview2.rs`. Validation is matrix H case H02 → **CANNOT VERIFY HERE** |
| 5.8 | Commit and version frozen | **MET** | Commit `0dda444` (`chore(release): freeze 2.0.0 RC`), local tag `v2.0.0-rc`, version 2.0.0 in all five manifests. The GA tag `v2.0.0` is deliberately withheld until matrix H passes (L-05) |

## 6. Security criteria

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 6.1 | No secret in the repository or its history | **MET** | The Linear API key was hardcoded in 42 scripts and committed in `e041918`. That commit was **never pushed**, so it was reset and the work re-committed clean as `1b46c87`. `git grep` over all reachable commits returns empty; `.env` is gitignored; `.env.example` added. **The key itself should still be rotated** — it existed in plaintext on disk |
| 6.2 | HTML sanitizer resists XSS | **MET** | `domain/sanitizer.rs` (1,942 LOC). `qa_m7_007_xss_payload_matrix_is_neutralized`, `qa_m7_008_dangerous_url_matrix_is_rejected`, `qa_m7_008_image_src_matrix_is_restricted_to_managed_paths`, `qa_m7_009_all_four_choke_points_sanitize`, plus `qa_m7_007_benign_formatting_survives_sanitation` guarding against over-sanitisation |
| 6.3 | Attachment paths cannot traverse the asset root | **MET** | `attachment::safe_join` / `resolve_path` reject traversal; covered by QA-M6-011~017 |
| 6.4 | Crash dumps are privacy-safe | **MET** | `diagnostics/crashlog.rs` — asserted that a path containing Chinese characters never appears verbatim; paths are hashed only. Live validation is matrix H case H13 → **CANNOT VERIFY HERE** |

## 7. Known open defect and verification debt

**Fixed during this work — real data loss:**
`XmlElement::text_content()` (`domain/xml_tree.rs`) collected only `XmlNode::Text` and ignored
`XmlNode::CData`. A CDATA-wrapped HTML comment — the pattern in the repo's own
`comments/html-comment.xml` fixture — therefore mapped to an **empty** domain comment, and because
session save rebuilds COMMENTS from `task.comments`, the user's comment text was **silently
destroyed on save**. Found by `qa_m10_a08`. Fixed by concatenating `Text` and `CData` in document
order; the assertion was changed from documenting the defect to requiring the fix.

**Fixed during this work — second real data-correctness bug:**
`set_field` in `domain/command.rs` wrote only the OLE float for `StartDate`/`DueDate`, never the
companion `start_date_string`/`due_date_string`, while `mappers::write_task` serialises **both**. So
every date edit made through the mandated `FieldUpdateCommand` path left `STARTDATESTRING` /
`DUEDATESTRING` stale in the saved XML, and legacy TDL kept displaying the pre-edit date. Undo was
wrong in the same way, because `get_field` captures only the float and undo replays through
`set_field`. Fixed by deriving the `YYYY-MM-DD` string from the OLE value inside `set_field`, which
corrects both directions at once. Found by the IPC bridge work; covered by four new regression tests
(`set_start_date_refreshes_display_string`, `set_due_date_refreshes_display_string`,
`clearing_a_date_clears_its_display_string`, `undo_restores_both_the_float_and_the_display_string`).

**Fixed during this work — third real data-loss bug, on the primary save path:**
`serialize_task_tree` (`commands/session.rs`) built a fresh `<TODOLIST NEXTUNIQUEID="1">` and emitted
only **root-level** tasks into brand-new empty `<TASK>` elements. Because `mappers::write_task`
preserves the nested `<TASK>` children already present on the element it is handed, passing an empty
element meant **every subtask was silently discarded on save**. It also dropped the original root
attributes, comments and unknown elements, and forced UTF-8 regardless of the source encoding. This
is the Phase 0.1 "simplified session serialization" gap, sitting on the path used by both `file.save`
and autosave. Fixed by retaining the parsed `XmlDocument` on `SessionEntry` and rebuilding from it:
original `<TASK>` elements are indexed by ID and updated in place, nested tasks are rebuilt
recursively from the tree, non-TASK nodes keep their positions, and the source encoding meta is
reused. Covered by five new in-module regression tests.

**Fixed during this work — fourth defect, a crash in search indexing:**
`strip_html` (`domain/search.rs`) indexed a `Vec<char>` with `i` but then used `i` to byte-slice the
source strings, so any HTML comment containing a multibyte character before a tag's closing `>`
panicked. Through `task_description_text` → `SearchDocument::from_task` this crashed
`rebuild_search_index` for any document carrying CJK rich text. Fixed by reconstructing the tag in
char space. `qa_m10_e06` had pinned the buggy behaviour as a tripwire and named the assertion to use
once fixed; it is now a regression test.

**Fixed during RC (2026-10-21) — fifth real defect, a GA-blocking session-open failure:**
`open_document_session` (`commands/session.rs`) read the path it was handed verbatim. The index
stores managed documents' paths **relative to the workspace root** and the frontend passes them over
verbatim, so every managed document failed to open with a file-not-found from the process CWD — in
the UI, "Descriptions are unavailable for this task" everywhere. Found by CDP runtime verification
of the M7 editor (absolute-path documents worked; managed ones did not). Fixed at `a586175` by
resolving relative paths against the open workspace root (same rule as the transfer commands) and
storing the absolute path on the session so save/flush never depend on CWD. Covered by two in-module
regression tests (`open_session_resolves_workspace_relative_path_against_root`,
`open_session_without_workspace_root_keeps_relative_path_and_fails`).

**Reported, not fixed:**
- **F4** — `DeleteTaskCommand::undo` (`domain/command.rs`) re-appends tasks at the end of the
  sibling list, so delete+undo churns sibling order. IDs and content are intact. Asserted and
  documented in `qa_m10_d03`.
- **F1/F2** — `xml_parser.rs:91` lets the XML declaration override BOM detection, so a UTF-8-BOM file
  declaring `utf-8` loses its BOM on re-save, and a UTF-16 file with a contradicting declaration is
  re-encoded to UTF-8. Pre-documented at the M2 gate; consistent files round-trip byte-identically.

**Resolved since the 9-22 audit:**
- ~~**Spec §0.1 gap** — `commands/session.rs::serialize_task_tree` drops nested subtrees and root
  attributes.~~ Fixed (third defect above); five in-module regression tests pin the behaviour.
- ~~**Tiptap is not integrated.**~~ INH-1061/INH-1062 are Done: `@tiptap/*` is a dependency,
  `src/components/editor/RichTextEditor.vue` ships edit/preview modes, and the M7 editor passed CDP
  runtime verification on 2026-10-21.
- ~~**Frontend has had no runtime verification.**~~ The packaged WebView2 app was driven over CDP
  on 2026-10-21: M9 UI smoke (Smart Views, Saved Views, shortcut settings), M8 transfer E2E
  (E1–E6), the RD-M10-022 render gate measurement (`docs/qa/evidence/m10-render-bench.json`), and
  a live index.db delete-rebuild. Evidence committed at `c293554` / `bcddfde`.

**Verification debt:**
- **RESOLVED — the 27 missing IPC commands.** `update_task_field`, `add_task`, `delete_task`,
  `global_search`, `quick_add_task` and every participant / dependency / progress-link / attachment
  command now exist in `commands/task_edit.rs` (7), `commands/relations.rs` (19) and
  `commands/bridge.rs` (2). `invoke_handler` registers **69 commands**. Field mutations route through
  `FieldUpdateCommand` + `UndoRedoManager`, so they are undoable and mark the session dirty for
  autosave; `status` maps to `percent_done` because `Task` has no status field. Relation commands are
  thin wrappers over the M6 domain APIs, so URL-scheme and path-traversal validation is neither
  duplicated nor weakened. Covered by `tests/ipc_bridge_tests.rs` (43 tests).
- **RESOLVED — rustc crash from the enlarged handler macro.** With ~69 commands the
  `generate_handler!` expansion overflowed the compiler stack and aborted the bin target with
  `0xc0000409 (STATUS_STACK_BUFFER_OVERRUN)`. Because the lib and every integration target compiled,
  this presented as a link failure. `src-tauri/.cargo/config.toml` now sets `RUST_MIN_STACK=64MiB`.
- **Tiptap is integrated** (see "Resolved since the 9-22 audit" above) — the 9-22 claim that
  INH-1061/1062 were unimplemented is superseded.
- **Frontend runtime verification is done** (see above) — the 9-22 "no runtime verification" debt is
  superseded by the 2026-10-21 CDP evidence.
- **RESOLVED — a single clean full-suite `cargo test` run is now captured: 1146 passed, 0 failed.**
  The earlier failures were **not** target-directory corruption. Root cause: `[lib] crate-type =
  ["staticlib", "cdylib", "rlib"]` (the Tauri template default). `staticlib`/`cdylib` are consumed
  only by the iOS/Android mobile entry points, and their presence made cargo build the dependency
  graph without the rlib metadata that `tests/*.rs` link against, producing `crate X required to be
  available in rlib format` and cascading `can't find crate for moderntodolist_lib`. It was easy to
  misdiagnose as corruption because `cargo test --lib` passed (581) while every integration target
  failed. Fixed by setting `crate-type = ["rlib"]`, which is correct for a Windows-only portable
  desktop app; the reasoning is recorded in `Cargo.toml`.

## 8. What would close the gate

1. Execute matrix H (15 cases) on clean Win10 and Win11 VMs against the frozen RC ZIP and record
   results — **human-only**, environment-blocked per `docs/qa/LEFTOVER_TASKS.md` L-04. If any case
   forces a code change, rebuild + re-freeze the ZIP (new SHA-256) and redo the render re-measure
   (L-02).
2. ~~Implement QA-M10 matrices E, F and G as standalone RC suites (32 cases).~~ **Done** — 32 cases,
   all passing.
3. ~~Integrate Tiptap and the rich-text editor UI (INH-1061, INH-1062).~~ **Done.**
4. ~~Perform real runtime UI verification of the 10 issues held at `In Progress`.~~ **Done** —
   CDP-driven, evidence committed.
5. ~~Apply the version metadata to `tauri.conf.json` and produce a frozen release ZIP.~~ **Done** —
   commit `0dda444`, tag `v2.0.0-rc`, SHA-256 frozen.
6. ~~Fix F4 and the `serialize_task_tree` subtree/attribute gap.~~ Partially done — the
   serialization gap is fixed; **F4** (delete+undo sibling order) remains a documented non-blocker
   (IDs/content intact).
7. ~~Re-run `rc-blocker-review.cjs` until the count is 0, then freeze commit, version and ZIP hash.~~
   **Done** — product blocker categories all 0; the 4 open items are process gates including this one.
8. After H passes: close this gate and epic INH-718, tag the GA commit `v2.0.0`, and rotate the
   Linear API key (§6.1 — it once existed in plaintext on disk).

**Do not close this gate on the strength of passing unit tests alone.** Criteria 3.H, 4.3 and 4.4
require physical clean-machine execution that no automated suite can substitute for.
