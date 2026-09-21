# External Dependency Policy for Cross-Document Transfers (M8)

> Implements: RD-M8-006~010 (INH-1079)
> Code: `src-tauri/src/domain/transfer.rs` (`ExternalDependencyPolicy`, `rewrite_dependency`, `validate_target_bytes`)
> Date: 2026-09-22

---

## 1. Definitions

For a transfer of a task subtree `S` (root task + all descendants) from a
SOURCE document to a TARGET document, every `<DEPENDENCY>` element inside `S`
is classified during planning:

| Class | Condition | Meaning |
|---|---|---|
| **Internal** | `<TASKID>` resolves to a task inside `S` | Both endpoints move together |
| **External** | `<TASKID>` does not resolve inside `S` | The other endpoint stays behind (or never existed) |

Classification is computed from the domain snapshot of the subtree
(`Task.dependencies` via `mappers::read_task`), then applied to the lossless
XML clone that is actually written into the target document.

## 2. Internal dependencies

Rewritten deterministically through the transfer ID map
(`TransferManifest.id_map`, source ID → newly allocated target ID). Both the
`<TASKID>` text of `<DEPENDENCY>` elements and every nested `<TASK ID>`
attribute are rewritten. Verified by `validate_target_bytes` before the
atomic replace: **every** dependency `<TASKID>` in the committed target must
either resolve inside the target document or carry a `<FILENAME>` external
marker (§3). An unresolved reference without a marker fails validation and
aborts the target commit (no partial state: the atomic-save pipeline leaves
the original target file intact).

## 3. External dependencies — the policy

The legacy TDL format has **no verified cross-file dependency syntax**
(`docs/compatibility/DEPENDENCY_FORMAT.md` §3 marks it TBD). Because
byte-level compatibility with AbstractSpoon TDL is a hard requirement, we
must not invent attributes on the `<DEPENDENCY>` element itself. The
implemented default therefore uses the *guessed* cross-file shape from that
audit document — a `<FILENAME>` **child element** — which is exactly the kind
of unknown child element that both AbstractSpoon-tolerant readers and our own
lossless M2 parser preserve verbatim.

### 3.1 `PreserveAsExternal` (DEFAULT — implemented)

For each external dependency inside the transferred subtree:

1. The `<TASKID>` value is kept **unchanged** (the numeric source-space ID).
2. A `<FILENAME>` child element is appended to the `<DEPENDENCY>` element
   (unless one already exists), containing the source document's file name.
   This marks the reference as cross-document, so no reader can mistake it
   for a same-document reference to an unrelated target task that happens to
   carry the same numeric ID.
3. A `TransferWarning::ExternalDependencyPreserved { task_id, dep_task_id,
   source_doc }` is emitted and surfaced in the transfer result / UI.

Rationale: the dependency is *data*. Silently dropping it loses information;
silently keeping a bare numeric ID creates a false intra-document reference.
Preserve-with-marker degrades the dependency to an Explicit External ref —
the same degradation model M6 uses for cross-document `TaskRef::External` —
while remaining round-trip safe in both directions.

The marker is written only into the TARGET copy. The SOURCE document is
never modified by this rule (and, for copies, not at all).

### 3.2 `Block` (opt-in — implemented)

Planning aborts with `TransferError::ExternalDependenciesBlocked` listing
every offending `(task, dependency)` pair. **Zero side effects**: no journal,
no asset staging, no file writes. Use when the user requires dependency
closure (e.g. moving a sub-project that must not reference the old document).

### 3.3 Rejected alternative: drop-with-warning

Dropping external `<DEPENDENCY>` elements was considered and **rejected**:
it destroys user data irreversibly in the target copy and violates the
project's data-preservation bias. No code path deletes dependency data.

## 4. Reverse references (move only)

When a MOVE removes subtree `S` from the source, dependencies held by
*remaining* source tasks that point INTO `S` become unresolved in the source
document. Policy:

- They are **preserved as-is** (never deleted): the source rewrite only
  removes `<TASK>` elements of `S`; it never edits other tasks' dependencies.
- Planning emits `TransferWarning::OrphanedSourceDependency` for each one.
- The M6 dependency layer renders unresolved refs in a degraded state.

This keeps the source rewrite minimal and byte-conservative, and matches
AbstractSpoon behaviour where deleting a task leaves its inbound references
unresolved.

## 5. Validation and testing

- `validate_target_bytes` (transfer.rs) enforces §2 on every target commit
  (both inline before `atomic_save` and inside its re-read validate hook).
- QA-M8-013 verifies internal remap (`6→5` becomes `5→4`, `7→6` becomes
  `6→5` on the transfer fixtures).
- QA-M8-014 verifies the default preserve path (TASKID kept, `<FILENAME>`
  marker added, warning emitted) and the Block path (planning error, target
  file byte-identical, no journal created).
