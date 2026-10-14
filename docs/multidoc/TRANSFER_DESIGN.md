# Multi-Document Transfer & File Library Design (M8)

> Implements: RD-M8-001~045 (INH-1077 ~ INH-1088), QA-M8-001~020 (INH-1089 ~ INH-1093)
> Modules: `src-tauri/src/domain/transfer.rs`, `transfer_journal.rs`, `file_library.rs`, `trash.rs`
> QA suite: `src-tauri/tests/m8_qa_tests.rs`
> Fixtures: `tests/fixtures/xml/transfer/`
> Companion doc: `docs/multidoc/EXTERNAL_DEPENDENCY_POLICY.md`
> Date: 2026-09-22

---

## 1. Goal and overriding invariant

Cross-document copy/move of task subtrees between TDL documents, plus a File
Library (create/rename/move/duplicate/import/link/trash) — under one rule
that dominates every design decision:

> **A task must NEVER be lost. An interrupted transfer must leave a
> DUPLICATE rather than a loss.** (GATE-M8, P0)

## 2. Architecture

```
plan_transfer ──► TransferPlan { TransferManifest, subtree XML clone, warnings }
execute_transfer ──► journal phases ──► target-first commit ──► guarded source delete
recover_interrupted_transfers ──► RolledBack | Completed | CompletedWithDuplicate | Quarantined
complete_interrupted_move ──► user-confirmed resolution of CompletedWithDuplicate
undo_transfer ──► byte-exact restore of pre-commit documents (one-shot)
```

All four modules operate on the EXISTING public domain API:
`parse_xml`/`serialize_xml` (M2 lossless tree), `mappers::read_task` (domain
snapshot), `TaskIdAllocator` (target ID space), `FileFingerprint` (BLAKE3),
`persistence::atomic_save` (M3 temp→validate→atomic-replace), and
`workspace::Workspace` (M4 workspace.json registry). Nothing in the transfer
path re-implements atomic save or fingerprinting.

### 2.1 Why the XML clone is the payload

The transferred payload is a deep clone of the source `<TASK>` **XmlElement**
(the lossless M2 tree), not a re-serialization of the domain `Task`. Unknown
attributes, unknown child elements, comments, CDATA, METADATA and element
ordering therefore survive byte-for-byte; only the deliberately rewritten
references change (IDs, dependencies, asset paths). The domain `TaskTree`
snapshot (`TransferPlan.subtree_snapshot`) is used for planning, dependency
classification, asset discovery and validation only.

### 2.2 ID allocation (target space)

Per `docs/compatibility/ID_ALLOCATION_RULES.md`:
- IDs come from the TARGET document's `NEXTUNIQUEID` via `TaskIdAllocator`
  (collision-skipping).
- Rule §5.3-4 correction: effective start = `max(declared NEXTUNIQUEID,
  max(numeric target ID)+1)`.
- The target root's `NEXTUNIQUEID` is written to the allocator's post-state.
- IDs are never recycled: transaction Undo restores pre-transfer document
  bytes but raises `NEXTUNIQUEID` back to the post-transfer floor.

### 2.3 Reference rewriting

- Nested `<TASK ID>` attributes → new target IDs (DFS order via `id_map`).
- `<DEPENDENCY><TASKID>`: internal → remapped; external → policy-driven
  (default `PreserveAsExternal`: keep value + add `<FILENAME>` marker +
  warn; opt-in `Block`: abort planning with zero side effects). See
  EXTERNAL_DEPENDENCY_POLICY.md.
- `<FILEREFPATH>` + HTML comment `src=`/`href=` refs: see §2.4.
- `POS` of the inserted root = count of existing root-level target tasks
  (append at end). `POSSTRING`/`REFID`/`LASTMOD` are preserved unchanged
  (Tier B: do not fabricate values).
- Unknown elements are scanned conservatively (`TASKID` text, nested `TASK`
  IDs, `FILEREFPATH`) so plugin payloads referencing task IDs stay coherent.

### 2.4 Asset plan (RD-M8-008~010)

Each reference is classified:

| Class | Rule | Action |
|---|---|---|
| `CopyRequired` | document-relative path, file exists | copy to same relative layout under the TARGET dir; hash-verify after copy |
| `ReferenceOnly` | URL (`http/https/file/ftp/mailto`) or absolute local path | reference kept verbatim + warning (progress links preserve URLs) |
| `Skip` | missing file, or byte-identical file already at destination | reference kept; warning when missing |

Collisions: a differing file already at the destination is NEVER clobbered —
the copy lands at `<stem>-<uuid8><ext>` and the XML reference is rewritten
(`AssetCollisionRenamed` warning). Staging is crash-idempotent: an identical
file already at the destination counts as staged (retry-safe).

## 3. Target-first commit protocol (RD-M8-011~019)

```
P0 plan            (side-effect free; fingerprints of both docs recorded)
P1 journal Started (durable, fsync'd BEFORE first side effect)
P2 stage assets    → journal AssetsStaged
P3 build target bytes; validate in-memory; atomic_save(target)   [M3 pipeline]
   → journal TargetCommitted (durable proof; authorizes ALL source mutations)
P4 copy:  → journal Completed → delete journal
   move:  backup source bytes (fsync) → journal SourceBackedUp
          re-read source; REVALIDATE fingerprint vs plan-time
            mismatch → abort with SourceConcurrentlyModified (duplicate kept)
          remove subtree; validate; atomic_save(source)
          → journal SourceCommitted → journal Completed → delete journal
```

Key properties:

- **Ordering**: the source is only touched after `TargetCommitted` is
  durable. `commit_source_deletion` re-checks that journal entry itself
  (defense in depth) before doing anything.
- **Journal durability**: every append = write tmp + `sync_all` + rename over
  the primary, keeping the previous version as `.bak`. Loading falls back to
  `.bak`; a journal whose primary AND bak are unreadable is `Corrupt`.
- **Validation**: target commits are validated twice (pre-write and via
  atomic_save's re-read hook): well-formed XML, every mapped ID present
  exactly once, expected task count, `NEXTUNIQUEID` above every numeric ID,
  and no unresolved dependency without a `<FILENAME>` marker.
- **Fingerprint revalidation** before source deletion detects concurrent
  external edits; on mismatch the source is left intact → duplicate, not loss.
- **Journal cleanup** happens only after `Completed` is durable.

## 4. Recovery (RD-M8-018~019)

`recover_interrupted_transfers(journal_dir)` scans
`<ws>/.moderntodo/transfer/*.journal.json` and classifies each transaction by
(journal phases × actual filesystem state), never trusting the journal alone
— the crash may have landed between the atomic replace and the journal
append:

| Journal / FS state | Outcome | Action |
|---|---|---|
| no `TargetCommitted`, target lacks mapped IDs | `RolledBack` | remove hash-verified staged assets; delete journal |
| `TargetCommitted` (or target actually contains IDs), Copy | `Completed` | finalize journal |
| committed target + source still holds subtree (Move) | `CompletedWithDuplicate` | **nothing deleted**; journal kept as evidence; duplicates listed in report |
| `SourceCommitted` present | `Completed` | finalize journal |
| journal corrupt (primary+bak) | `Quarantined` | untouched; surfaced for triage |

Recovery NEVER deletes task data. The `CompletedWithDuplicate` state is
resolved only by explicit user action: `complete_interrupted_move()` re-runs
the guarded source-delete phase (durable-`TargetCommitted` gate + fingerprint
revalidation + fresh backup), or the user keeps/removes the duplicate
manually. Staged-asset cleanup only deletes files whose BLAKE3 hash matches
the journal's record (never user data).

## 5. Services and UX backend (RD-M8-020~031)

- `copy_task(...)` — plan + pipeline; the source is read-only throughout.
- `move_task(...)` — plan + pipeline + guarded source-delete phase.
- `undo_transfer(ctx, txn)` — transaction Undo from durable byte-exact
  backups under `<ws>/.moderntodo/transfer-undo/<txn>/` (`target.before.xml`,
  `source.before.xml`, `undo.json`). Copy undo removes the target subtree;
  Move undo restores both documents. One-shot (`AlreadyUndone` on replay).
- `TransferError` — thiserror enum with a distinct variant per failure mode
  (read/parse/not-found, same-document, blocked externals, asset copy,
  journal, concurrency on either doc, target/source commit, validation,
  undo, simulated kill, recovery precondition, I/O).
- `list_transfer_targets(ws, exclude)` — target-picker data: every registered
  document with resolved path, type and a real writability probe.
- `TransferProgress` events flow through a callback for the progress
  indicator; `TransferRecoveryReport` (action, detail, duplicate IDs) feeds
  the post-crash Recovery results display.
- QA seams: `TransferHooks { fail_before: Option<TransferPhase> }` injects a
  deterministic kill at each phase boundary (`SimulatedKill`).

## 6. File Library (RD-M8-032~045)

State lives in `workspace.json` (M4 `Workspace`) plus
`<ws>/.moderntodo/file-library/` (receipts + repair backups) and
`<ws>/.moderntodo/trash/`.

- **Create Managed** (`create_managed_document`): writes a minimal valid TDL
  (`NEXTUNIQUEID="1"`, UTF-8, CRLF) through `atomic_save`, registers with a
  fresh stable `DocumentId`, records a fingerprint baseline.
- **Rename / Move** (`rename_or_move_document`): physical `fs::rename` +
  in-place entry path update — the `DocumentId` NEVER changes. Afterwards
  **path-reference repair** (§7) rewrites `<FILEREFPATH>` refs in all other
  registered documents.
- **Duplicate** (`duplicate_document`): byte copy with `FILENAME` root attr
  updated; original keeps its ID, duplicate gets a new one; registration is
  rolled back if the commit fails.
- **Import as Managed** (`import_as_managed`): copy-in (source never
  modified), parse-validated first. **Link External** (`link_external`):
  absolute-path registration in place. **Remove Reference**
  (`remove_reference`): unregister ONLY — the file is never deleted here.
- **Reveal in Explorer** (`reveal_command` / `reveal_in_explorer`): builds
  `explorer /select,<path>` (platform-guarded); exit code ignored (explorer
  quirk), spawn failure is an error.
- **Undo policy** (`undo_document_op`, receipts under
  `.moderntodo/file-library/receipts/`):

| Op | Undo |
|---|---|
| Create / Duplicate / ImportManaged | unregister + **delete-to-trash** (never silent hard delete) |
| Rename / Move | move file back + restore entry path + byte-exact restore of every reference-repaired document |
| RemoveReference | re-register the ORIGINAL entry (stable DocumentId) |

## 7. Path-reference repair (RD-M8-045)

After a managed rename/move, every OTHER registered document is parsed;
`<FILEREFPATH>` texts that resolve (relative to the referencing doc, or
absolute) to the moved file's old location are rewritten to the new location
(relative when under the referencing doc's directory, else absolute; Windows
case-insensitive match). Each rewritten document is committed via
`atomic_save`, and its pre-repair bytes are backed up under
`.moderntodo/file-library/repair-backups/` so the receipt-based undo restores
them byte-exactly.

## 8. Trash (RD-M8-039~042)

`<ws>/.moderntodo/trash/{manifest.json, files/<trash_id>__<name>}`.

- **delete-to-trash** (`move_file_to_trash[_ws]`,
  `delete_document_to_trash`): fingerprint → move (rename, or
  copy+verify+delete across volumes) → verify stored hash → durable manifest
  (tmp+fsync+rename, `.bak` fallback) → only then unregister from the
  workspace. On verification failure the file is put back and nothing else
  changes.
- **restore**: integrity check → copy back to the ORIGINAL path
  (collision-safe `(restored-N)` suffix) → verify → remove stored copy +
  manifest entry → re-register with the ORIGINAL stable `DocumentId`.
- **purge / empty_trash**: the only paths that permanently destroy bytes;
  explicit user actions.

## 9. Crash matrix (QA-M8-001~012) and where each kill lands

| Kill point (Move) | Journal state at recovery | Recovery outcome | Data state |
|---|---|---|---|
| before StageAssets | none | (nothing to recover) | both docs untouched |
| before CommitTarget | Started+AssetsStaged | RolledBack | both docs untouched; staged copies removed |
| before BackupSource | TargetCommitted | CompletedWithDuplicate | subtree in BOTH docs |
| before CommitSource | +SourceBackedUp | CompletedWithDuplicate | subtree in BOTH docs; source backup durable |
| before Complete | +SourceCommitted | Completed | final state, journal cleaned |

Every row is asserted in `m8_qa_tests.rs` with the union-no-loss check:
all original titles of both documents exist in `source ∪ target` after
recovery. Journal corruption (garbled primary+bak → `Quarantined`), torn
journal writes (`.bak` fallback), target write failure (occupied temp path),
permission denied (read-only target), asset-stage I/O failure (destination
occupied by a directory) and post-commit source edits
(`SourceConcurrentlyModified`, duplicate kept) complete the matrix.

## 10. Known limitations / orchestrator notes

- `LASTMOD`/`EARLIESTDUEDATE` root attrs and `POSSTRING` values of inserted
  tasks are preserved, not recomputed (Tier B: no fabricated values; TDL
  recomputes POSSTRING on next full save by AbstractSpoon).
- M6/M7 relation modules (participants/attachments/progress links as domain
  types) are being landed concurrently; the transfer layer is deliberately
  built on the M2 `Task` surface (`allocated_to`, `file_links`,
  `dependencies`, `comments`) + lossless XML so it stays correct regardless:
  ANY child-element payload inside the subtree travels with the clone.
- The workspace SQLite index is not updated by the transfer layer; after a
  commit the orchestrator should trigger the existing re-index path
  (`scan_and_index` / watcher) for both documents.
- Recovery is startup-driven: call `recover_interrupted_transfers` at
  workspace open (suggested wiring in `commands/workspace.rs`).
