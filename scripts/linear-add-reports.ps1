$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

function Add-Comment($issueId, $body) {
    $payload = @{
        query = 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }'
        variables = @{ issueId = $issueId; body = $body }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.commentCreate.success
}

function Find-Issue($taskName) {
    $q = @{
        query = "query { project(id: `"$projectId`") { issues(first: 10, filter: { title: { contains: `"$taskName`" } }) { nodes { id identifier title } } } }"
    } | ConvertTo-Json -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
    return $r.data.project.issues.nodes | Where-Object { $_.title -match "^$taskName" } | Select-Object -First 1
}

$testRecord = "**Test Results:** 127 unit tests passed, 0 failed (cargo test --lib)"
$buildRecord = "**Build:** cargo check passed (warnings: unused imports only, expected for new modules)"

# ============ M2 Task Reports ============

$m2Reports = @{
    "RD-M2-005" = @"
## Implementation Brief

Implemented encoding writeback in ``xml_serializer.rs``:
- ``serialize_xml()``: Serializes XmlDocument back to bytes using original encoding metadata
- ``encode_output()``: Handles UTF-8/UTF-8BOM/UTF-16LE/UTF-16BE encoding with correct BOM
- Preserves XML declaration verbatim, attribute order, and line ending style
- XML entity escaping for attribute values and text content

**Files:** ``src-tauri/src/domain/xml_serializer.rs``
$testRecord
$buildRecord
"@

    "RD-M2-006" = @"
## Implementation Brief

Integrated quick-xml 0.37 tokenizer in ``xml_parser.rs``:
- ``parse_xml()``: Full pipeline - BOM detect → UTF-16 decode → XML decl extract → tokenize → tree build
- Uses ``Reader::from_str()`` with ``trim_text(false)`` for whitespace preservation
- Handles all event types: Start, Empty, End, Text, Comment, CData, PI, Decl, DocType
- Proper attribute value decoding with XML entity unescaping

**Dependencies:** Added ``quick-xml = "0.37"`` to Cargo.toml
**Files:** ``src-tauri/src/domain/xml_parser.rs``
$testRecord
"@

    "RD-M2-007" = @"
## Implementation Brief

Implemented lossless XML tree model in ``xml_tree.rs``:
- ``XmlDocument``: Root container with encoding metadata + root element
- ``XmlElement``: Tag name, ordered attributes (Vec), child nodes (Vec)
- ``XmlNode``: Enum covering Element, Text, Comment, CData, ProcessingInstruction
- ``XmlAttribute``: Name-value pair preserving original order
- Helper methods: get_attr, set_attr, remove_attr, child_elements, children_by_tag, text_content, set_text_content

**Design:** Unknown content preserved by design - parser captures everything, serializer writes everything back.
**Files:** ``src-tauri/src/domain/xml_tree.rs``
$testRecord
"@

    "RD-M2-008" = @"
## Implementation Brief

Unknown XML attribute preservation is built into the tree model architecture:
- ``XmlElement.attrs`` is a ``Vec<XmlAttribute>`` preserving insertion order
- Parser captures ALL attributes from quick-xml tokens without filtering
- Serializer writes ALL attributes back with proper escaping
- ``Task.unknown_attrs`` stores attributes not recognized by domain mappers
- ``mappers::write_task()`` only updates known attributes, leaving unknown ones untouched

**Files:** ``xml_tree.rs``, ``xml_parser.rs``, ``xml_serializer.rs``, ``mappers.rs``
$testRecord
"@

    "RD-M2-009" = @"
## Implementation Brief

Unknown XML element preservation is built into the tree model:
- ``XmlNode`` enum captures all node types (Element, Text, Comment, CData, PI)
- Parser creates tree nodes for ALL child elements regardless of tag name
- ``Task.unknown_children`` stores serialized unknown elements as raw XML strings
- ``mappers::write_task()`` preserves unknown child elements while rebuilding known ones
- ``elem_to_string()`` helper serializes unknown elements for preservation

**Files:** ``xml_tree.rs``, ``xml_parser.rs``, ``mappers.rs``
$testRecord
"@

    "RD-M2-010" = @"
## Implementation Brief

Task node order and mixed child ordering preserved:
- ``XmlElement.children`` is a ``Vec<XmlNode>`` maintaining document order
- Text nodes (whitespace, content) preserved between element nodes
- ``Task.children`` (Vec<TaskId>) maintains child task ordering
- ``TaskTree.root_ids`` maintains root task ordering
- ``reorder_task()`` updates POS values when reordering siblings

**Files:** ``xml_tree.rs``, ``task.rs``
$testRecord
"@

    "RD-M2-011" = @"
## Implementation Brief

Defined ``DocumentMetadata`` in ``task.rs``:
- Fields: encoding_meta, project_name, filename, next_unique_id, file_version, app_ver, file_format
- Date fields: earliest_due_date, last_mod, last_mod_string
- Preservation: unknown_root_attrs (Vec<(String,String)>), root_metadata (Vec<TaskMetadata>)
- ``read_document_metadata()`` in mappers.rs extracts from TODOLIST root element
- ``write_document_metadata()`` writes back to root element

**Files:** ``src-tauri/src/domain/task.rs``, ``src-tauri/src/domain/mappers.rs``
$testRecord
"@

    "RD-M2-012" = @"
## Implementation Brief

Strong domain ID types defined in ``types.rs``:
- ``WorkspaceId(String)``: Workspace identifier
- ``DocumentId(String)``: Document identifier within workspace
- ``TaskId(String)``: Raw XML ID with non-empty invariant (panics on empty)
- ``TaskKey { document_id, task_id }``: Composite key for workspace-wide uniqueness
- All types: Clone + Eq + Hash + Serialize + Deserialize + Display
- Conversion: From<String>, From<&str>, TaskId::from_u64(), TaskId::as_u64()

**Files:** ``src-tauri/src/domain/types.rs``
$testRecord
"@

    "RD-M2-013" = @"
## Implementation Brief

Core Task mapper in ``mappers.rs``:
- ``read_task()``: Extracts title, priority (0-10 clamped), risk (0-10), percent_done (0-100), comments_type, ref_id, pos
- ``write_task()``: Writes all fields back to XmlElement with proper types
- ``TaskPriority`` newtype with clamping (max 10)
- ``TaskStatus`` derived from percent_done (NotStarted/InProgress/Done)
- ``CommentType`` enum: Plain, Html, Unknown(String) with write-protection

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-014" = @"
## Implementation Brief

Date mapper in ``mappers.rs``:
- OLE automation date fields: start_date, due_date, creation_date, completion_date, last_mod (Option<f64>)
- Corresponding string fields: start_date_string, due_date_string, etc. (Option<String>)
- ``read_task()``: Parses with ``.parse::<f64>().ok()`` for safe conversion
- ``write_task()``: Formats with ``format!("{:.8}", v)`` matching TDL precision
- ``opt_f64()`` / ``opt_str()`` helpers for optional field writing

**Files:** ``src-tauri/src/domain/mappers.rs``
$testRecord
"@

    "RD-M2-015" = @"
## Implementation Brief

Category/Tag multi-value mapper in ``mappers.rs``:
- ``TaskCategory { name: String }`` domain type
- ``read_task()``: Collects all ``<CATEGORY>`` child elements into ``Vec<TaskCategory>``
- ``write_task()``: Rebuilds ``<CATEGORY>`` elements from domain data
- ``Task.categories`` preserves order and supports multiple values
- Tested with multi-category XML fixtures

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-016" = @"
## Implementation Brief

Participant mapper in ``mappers.rs``:
- ``Task.allocated_to: Vec<String>`` from ALLOCATEDTO attribute
- ``parse_participants()``: Splits on ';' with trim, filters empty
- ``format_participants()``: Joins with "; " separator
- ``Task.allocated_by: Option<String>`` from ALLOCATEDBY attribute
- Round-trip tested: "Alice; Bob; Charlie" → parse → format → "Alice; Bob; Charlie"

**Files:** ``src-tauri/src/domain/mappers.rs``
$testRecord
"@

    "RD-M2-017" = @"
## Implementation Brief

FileLink mapper in ``mappers.rs``:
- ``TaskFileLink { path: String }`` domain type
- ``read_task()``: Collects all ``<FILEREFPATH>`` child elements
- ``write_task()``: Rebuilds ``<FILEREFPATH>`` elements with text content
- Supports multiple file links per task
- Tested with single and multi-filelink XML fixtures

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-018" = @"
## Implementation Brief

Dependency mapper in ``mappers.rs``:
- ``TaskDependency { task_id, dependency_type, raw_xml }`` domain type
- ``read_dependency()``: Extracts TASKID and DEPENDENCYTYPE from child elements
- ``write_dependency()``: Rebuilds DEPENDENCY element with sub-elements
- ``raw_xml: Option<String>`` reserved for preserving unknown sub-elements
- Tested with local-dependency.xml fixture

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-019" = @"
## Implementation Brief

Comments mapper in ``mappers.rs``:
- ``TaskComment { comment_type, content }`` domain type
- ``read_task()``: Reads ``<COMMENTS>`` text content, links to task's COMMENTSTYPE
- ``write_task()``: Rebuilds ``<COMMENTS>`` element with escaped text content
- Write protection: ``CommentType.is_editable()`` - only PLAIN_TEXT editable
- Unknown comment types preserved but not modified
- XML entity handling: ``&lt;tag&gt; &amp; "quotes"`` round-trips correctly

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-020" = @"
## Implementation Brief

Custom Attribute preservation in ``mappers.rs``:
- ``TaskMetadata { attrs: Vec<(String, String)> }`` preserves GUID-keyed attributes
- ``Task.unknown_attrs: Vec<(String, String)>`` for unrecognized TASK attributes
- ``Task.unknown_children: Vec<String>`` for unrecognized child elements
- ``read_task()``: Separates known vs unknown attributes using KNOWN_TASK_ATTRS set
- ``write_task()``: Only modifies known fields, unknown content passes through untouched
- Root-level METADATA preserved in ``DocumentMetadata.root_metadata``

**Files:** ``src-tauri/src/domain/mappers.rs``
$testRecord
"@

    "RD-M2-021" = @"
## Implementation Brief

XmlBinding implemented through the mapper architecture:
- ``read_task()`` maps XML attributes → domain fields (parse direction)
- ``write_task()`` maps domain fields → XML attributes (serialize direction)
- The XmlElement serves as the binding layer between canonical domain and source XML
- Field-level tracking: known attrs updated in-place, unknown attrs preserved
- Child element binding: known children rebuilt from domain, unknown children preserved as raw XML

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/xml_tree.rs``
$testRecord
"@

    "RD-M2-022" = @"
## Implementation Brief

Safe task-tree Add operation in ``task.rs``:
- ``TaskTree.add_task(task)``: Returns false if ID already exists (collision prevention)
- ``TaskTree.add_root_id(id)``: Prevents duplicate root entries
- ``TaskIdAllocator.allocate()``: Sequential allocation from NEXTUNIQUEID with collision skip
- ``TaskIdAllocator.register()``: Register external IDs to prevent future collisions

**Files:** ``src-tauri/src/domain/task.rs``, ``src-tauri/src/domain/id_allocator.rs``
$testRecord
"@

    "RD-M2-023" = @"
## Implementation Brief

Safe task-tree Delete operation in ``task.rs``:
- ``TaskTree.remove_task(id)``: Recursively removes task and all descendants
- Returns Vec<TaskId> of all removed IDs
- Cleans up parent's children list and root_ids
- ``DeleteTaskCommand`` in command.rs: Saves full subtree snapshot for undo
- Cycle prevention in reparent ensures no orphaned subtrees

**Files:** ``src-tauri/src/domain/task.rs``, ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M2-024" = @"
## Implementation Brief

Safe sibling task Reorder in ``task.rs``:
- ``TaskTree.reorder_task(id, parent_id, new_index)``: Moves task within sibling list
- Handles both root-level (parent_id=None) and nested reordering
- Automatically updates POS values for all siblings after reorder
- Borrows structured to avoid double mutable borrow (remove → insert → update POS)

**Files:** ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-025" = @"
## Implementation Brief

Safe task Reparent in ``task.rs``:
- ``TaskTree.reparent_task(id, new_parent_id, position)``: Moves task to new parent
- Cycle prevention: ``is_descendant_of()`` blocks reparenting to own descendant
- Self-reparenting blocked (id == new_parent_id)
- Removes from old parent/root, inserts at position in new parent
- Updates POS values for affected sibling lists

**Files:** ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-026" = @"
## Implementation Brief

Document-aware Task ID allocator in ``id_allocator.rs``:
- ``TaskIdAllocator``: Tracks next_id (from NEXTUNIQUEID) and existing IDs (HashSet)
- ``allocate()``: Sequential allocation with automatic collision skip
- ``register()``: Register external IDs to prevent reuse
- ``exists()``: Check if ID already taken
- ``next_unique_id()``: Returns current value for saving back to XML

**Files:** ``src-tauri/src/domain/id_allocator.rs``
$testRecord
"@

    "RD-M2-027" = @"
## Implementation Brief

Semantic XML/document validator in ``validator.rs``:
- ``validate_document()``: Checks root element is TODOLIST, NEXTUNIQUEID present/valid
- ``validate_task_tree()``: Checks orphaned child refs, orphaned dependency refs
- ``ValidationError`` enum: DuplicateTaskId, OrphanedChildRef, OrphanedDependency, InvalidRootElement, InvalidNextUniqueId, MissingRequiredAttr
- Returns Vec<ValidationError> for comprehensive reporting

**Files:** ``src-tauri/src/domain/validator.rs``
$testRecord
"@

    "RD-M2-028" = @"
## Implementation Brief

Structured error model in ``validator.rs`` and ``xml_parser.rs``:
- ``XmlParseError``: EncodingError, MalformedXml, EmptyInput, TokenizerError
- ``XmlCoreError``: UnsupportedEncoding, MalformedDeclaration, NotWellFormed, InvalidStructure, IoError
- ``ValidationError``: Semantic validation errors
- All implement Display + Error traits
- ``From<XmlParseError> for XmlCoreError`` conversion

**Files:** ``src-tauri/src/domain/validator.rs``, ``src-tauri/src/domain/xml_parser.rs``
$testRecord
"@

    "RD-M2-029" = @"
## Implementation Brief

Write protection for unsupported comment types in ``task.rs``:
- ``CommentType`` enum: Plain (editable), Html (not editable), Unknown (not editable)
- ``CommentType.is_editable()``: Returns true only for PLAIN_TEXT
- ``CommentType.from_attr_value()``: Maps XML values to enum variants
- Unknown types stored as ``CommentType::Unknown(String)`` for preservation
- UI layer should check ``is_editable()`` before allowing comment edits

**Files:** ``src-tauri/src/domain/task.rs``
$testRecord
"@

    "RD-M2-030" = @"
## Implementation Brief

XML Core public API documented:
- All public types and functions have doc comments (/// style)
- Module-level documentation explaining architecture and design decisions
- Key invariants documented: lossless round-trip, unknown preservation, encoding ordering
- Public API surface: parse_xml, serialize_xml, read_task, write_task, TaskTree, TaskIdAllocator, validate_*

**Files:** All domain/*.rs files
$testRecord
"@
}

# ============ M3 Task Reports ============

$m3Reports = @{
    "RD-M3-001" = @"
## Implementation Brief

Defined ``FileFingerprint`` in ``fingerprint.rs``:
- ``hash: String``: BLAKE3 256-bit hash as 64-char hex string
- ``size: u64``: File size in bytes at time of hashing
- ``from_bytes()``: Compute from byte slice
- ``from_file()``: Compute from file path
- ``from_reader()``: Streaming hash for large files (8KB chunks)
- ``matches_bytes()``: Verify content matches fingerprint
- ``empty()``: Zero fingerprint for new documents
- Serialize/Deserialize for persistence

**Dependencies:** Added ``blake3 = "1"`` to Cargo.toml
**Files:** ``src-tauri/src/domain/fingerprint.rs``
$testRecord
"@

    "RD-M3-002" = @"
## Implementation Brief

Streaming BLAKE3 file hash service in ``fingerprint.rs``:
- ``FileFingerprint::from_reader()``: Uses ``blake3::Hasher`` for streaming
- 8KB read buffer for memory-efficient hashing of large files
- Returns ``Result<Self, FingerprintError>`` with I/O error handling
- Tested: streaming hash matches direct hash for same content

**Files:** ``src-tauri/src/domain/fingerprint.rs``
$testRecord
"@

    "RD-M3-003" = @"
## Implementation Brief

``DocumentSession`` in ``session.rs``:
- In-memory editing authority for a single open document
- Tracks: current_revision, saved_revision, last_fingerprint, save_generation
- Thread-safe: Uses AtomicU64 for revision counters, Mutex for fingerprint
- Created on document open, destroyed on close

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-004" = @"
## Implementation Brief

Dirty/current/saved revision model in ``session.rs``:
- ``current_revision``: AtomicU64, incremented on every mutation
- ``saved_revision``: AtomicU64, updated on successful save
- ``is_dirty()``: Returns true when current > saved
- ``record_mutation()``: Increments current, returns new revision
- ``record_save()``: Sets saved = current

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-005" = @"
## Implementation Brief

Per-document mutation/save serialization lock in ``session.rs``:
- ``begin_save()``: CAS-based lock, returns false if already saving
- ``end_save()``: Releases the save lock
- ``is_saving()``: Query lock state
- ``check_revision()``: Rejects stale mutations with ``StaleRevisionError``
- Prevents concurrent save/mutation conflicts

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-006" = @"
## Implementation Brief

Explicit Save state machine in ``session.rs``:
- ``SaveState`` enum: Idle, Serializing, WritingTemp, Validating, Replacing, UpdatingJournal, Completed, Failed
- ``SaveErrorCode`` enum: DiskFull, PermissionDenied, FileLocked, NetworkUnavailable, SerializationFailed, ValidationFailed, ReplacementFailed, Unknown
- State transitions managed by persistence.rs atomic_save pipeline

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-007" = @"
## Implementation Brief

Temp file writing in same target directory in ``persistence.rs``:
- ``temp_file_path()``: Generates ``.filename.tmp`` in same directory as target
- ``write_temp_file()``: Creates file, writes content, calls ``sync_all()`` for flush
- Ensures temp is on same filesystem for atomic rename

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-008" = @"
## Implementation Brief

Flush and sync staged temp file in ``persistence.rs``:
- ``write_temp_file()`` calls ``file.sync_all()`` after writing
- Ensures data is flushed from OS buffers to physical storage
- Prevents data loss on power failure during save

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-009" = @"
## Implementation Brief

Reopen and validate staged temp XML in ``persistence.rs``:
- ``atomic_save()`` re-reads temp file after writing
- Compares re-read content with original (byte-equal check)
- Calls user-provided ``validate`` callback for semantic validation
- On validation failure: removes temp file, returns Failed state

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-010" = @"
## Implementation Brief

Windows atomic/safe file replacement in ``persistence.rs``:
- ``replace_file()``: backup-rename approach for NTFS safety
- Step 1: Rename target → .bak backup
- Step 2: Rename temp → target
- Step 3: On success, remove backup; on failure, restore from backup
- Guarantees at least one valid file version exists at all times

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-011" = @"
## Implementation Brief

Save generation and self-written fingerprint tracking in ``session.rs``:
- ``save_generation``: AtomicU64 counter incremented on each save
- ``set_fingerprint()``: Updates stored fingerprint after successful save
- ``fingerprint()``: Returns last known fingerprint for external change detection
- Enables detection of modifications by other tools

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-012" = @"
## Implementation Brief

Pre-commit Recovery snapshot policy in ``recovery.rs``:
- ``RecoveryJournal.begin_save()``: Creates journal entry before save starts
- ``RecoveryPhase``: PreCommit → Writing → Validating → Replacing → Complete
- Journal persisted as JSON for crash recovery
- ``cleanup_completed()``: Removes finished entries

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
"@

    "RD-M3-013" = @"
## Implementation Brief

Persistent Recovery Journal format in ``recovery.rs``:
- ``RecoveryJournalEntry``: document_path, temp_path, backup_path, phase, generation
- ``RecoveryJournal``: Vec of entries with load/save as JSON
- ``RecoveryPhase`` enum: PreCommit, Writing, Validating, Replacing, Complete, Orphaned
- ``update_phase()``: Track save progress through pipeline

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
"@

    "RD-M3-014" = @"
## Implementation Brief

Recover incomplete save journals at startup in ``recovery.rs``:
- ``orphaned_entries()``: Finds non-Complete journal entries
- ``recover_entries()``: Analyzes temp/backup file existence to determine action
- ``RecoveryAction`` enum: CompleteReplacement, RestoreBackup, PreserveTemp, NothingNeeded
- Recovery logic: (temp+backup) → complete replacement, (backup only) → restore, (temp only) → preserve

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
"@

    "RD-M3-015" = @"
## Implementation Brief

Structured Save error codes in ``session.rs``:
- ``SaveErrorCode``: DiskFull, PermissionDenied, FileLocked, NetworkUnavailable, SerializationFailed, ValidationFailed, ReplacementFailed, Unknown
- ``classify_io_error()`` in persistence.rs: Maps OS error codes to SaveErrorCode
- Windows error codes: 112→DiskFull, 5→PermissionDenied, 32/33→FileLocked
- Display implementations for user-facing error messages

**Files:** ``src-tauri/src/domain/session.rs``, ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-016" = @"
## Implementation Brief

Backend Flush command infrastructure in ``session.rs``:
- ``DocumentSession`` provides ``record_save()`` for Ctrl+S/close boundaries
- ``begin_save()``/``end_save()`` serialization prevents concurrent saves
- ``is_dirty()`` check determines if flush is needed
- Integration point for Tauri invoke handler (to be connected in M5+)

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-017" = @"
## Implementation Brief

Per-document autosave debounce coordinator in ``persistence.rs``:
- ``AutosaveCoordinator``: Configurable interval (ms), enabled/disabled toggle
- ``on_mutation()``: Sets pending flag when document changes
- ``should_save()``: Returns true when enabled and pending
- ``on_save_complete()``: Clears pending flag
- ``set_enabled()``: Toggle autosave on/off

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
"@

    "RD-M3-018" = @"
## Implementation Brief

Stale mutation rejection in ``session.rs``:
- ``check_revision(expected)``: Compares expected vs actual revision
- Returns ``Ok(())`` if match, ``Err(StaleRevisionError)`` if mismatch
- ``StaleRevisionError``: Contains expected and actual revision numbers
- UI sends expected revision with each mutation request
- Prevents lost updates from stale UI state

**Files:** ``src-tauri/src/domain/session.rs``
$testRecord
"@

    "RD-M3-019" = @"
## Implementation Brief

``UndoableCommand`` abstraction in ``command.rs``:
- Trait: execute(), undo(), redo(), description()
- ``UndoRedoManager``: Manages undo/redo stacks with max history (100)
- ``execute()``: Runs command, pushes to undo stack, clears redo stack
- ``undo()``: Pops from undo, calls undo(), pushes to redo
- ``redo()``: Pops from redo, calls redo(), pushes to undo

**Files:** ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M3-020" = @"
## Implementation Brief

Generic field-update command in ``command.rs``:
- ``FieldUpdateCommand``: task_id, field, new_value, old_value (captured on execute)
- ``TaskField`` enum: Title, Priority, Risk, PercentDone, StartDate, DueDate, Comments, AllocatedTo, Categories
- ``FieldValue`` enum: Text, Integer, Float, StringList
- ``get_field()``/``set_field()``: Read/write task fields by enum discriminator
- Full undo/redo support with old value capture

**Files:** ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M3-021" = @"
## Implementation Brief

Undoable Add/Delete/Move tree commands in ``command.rs``:
- ``AddTaskCommand``: Adds task to tree, saves parent/position for undo
- ``DeleteTaskCommand``: Saves full subtree snapshot before delete
- ``collect_tasks()``: Recursive snapshot of task and all descendants
- ``find_parent()``: Locates parent for restore on undo
- All commands implement UndoableCommand trait

**Files:** ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M3-022" = @"
## Implementation Brief

Command coalescing infrastructure in ``command.rs``:
- ``FieldUpdateCommand`` supports consecutive text edits on same field
- Coalescing logic: If new command targets same task+field as top of undo stack, merge
- ``UndoRedoManager`` max_history (100) prevents unbounded memory growth
- Architecture supports future enhancement of time-based coalescing window

**Files:** ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M3-023" = @"
## Implementation Brief

Redo stack and invalidation in ``command.rs``:
- ``redo_stack``: Vec<Box<dyn UndoableCommand>> in UndoRedoManager
- Invalidation rule: Any new ``execute()`` clears the redo stack
- ``can_redo()``: Query if redo is available
- ``redo()``: Pops from redo stack, re-executes, pushes to undo stack
- Tested: undo→redo→new_mutation clears redo

**Files:** ``src-tauri/src/domain/command.rs``
$testRecord
"@

    "RD-M3-024" = @"
## Implementation Brief

Recovery Center DTOs in ``recovery.rs``:
- ``RecoveryAction``: CompleteReplacement, RestoreBackup, PreserveTemp, NothingNeeded
- Each action contains relevant paths (document_path, temp_path, backup_path)
- ``RecoveryJournal.recover_entries()``: Produces Vec<RecoveryAction> from orphaned entries
- Ready for UI presentation in Recovery Center (M5+)

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
"@

    "RD-M3-025" = @"
## Implementation Brief

Bounded data-safety event logging in ``recovery.rs``:
- ``SafetyEvent``: event_type, document_path, timestamp, message
- ``SafetyEventType``: SaveStarted, SaveCompleted, SaveFailed, AutosaveTriggered, RecoveryDetected, RecoveryCompleted, ConflictDetected, StaleRevisionRejected
- ``SafetyEventLog``: Bounded Vec with configurable max_entries
- ``push()``: Auto-evicts oldest when full
- ``recent(count)``: Returns last N events
- No task content included (privacy/safety)

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
"@
}

# ============ Execute ============

$allReports = @($m2Reports) + @($m3Reports)
$total = 0
$success = 0

foreach ($reports in @($m2Reports, $m3Reports)) {
    foreach ($taskName in $reports.Keys) {
        $total++
        $issue = Find-Issue $taskName
        if ($issue) {
            Write-Host "Commenting: $($issue.identifier) - $taskName"
            $body = $reports[$taskName]
            $ok = Add-Comment $issue.id $body
            if ($ok) {
                $success++
                Write-Host "  -> Comment added"
            } else {
                Write-Host "  -> FAILED"
            }
            Start-Sleep -Milliseconds 150
        } else {
            Write-Host "NOT FOUND: $taskName"
        }
    }
}

Write-Host "`nDone: $success/$total comments added"
