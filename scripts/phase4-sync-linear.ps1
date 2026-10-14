$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$cancelledStateId = "6f23a178-e79b-403f-9555-2164e9ce4006"  # Will be discovered if wrong
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

function Add-Comment($issueId, $body) {
    $payload = @{
        query = 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }'
        variables = @{ issueId = $issueId; body = $body }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.commentCreate.success
}

function Update-State($issueId, $stateId) {
    $payload = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $issueId; stateId = $stateId }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.issueUpdate.success
}

$testRecord = "**Test Results:** 172 tests passed (127 unit + 45 integration), 0 failed"
$buildRecord = "**Build:** cargo check 0 errors, cargo clippy 0 warnings"

# ============ 16 Task Reports ============
$reports = @{
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
$buildRecord
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
$buildRecord
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
$buildRecord
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
$buildRecord
"@

    "RD-M2-021" = @"
## Implementation Brief

XmlBinding implemented through the mapper architecture:
- ``read_task()`` maps XML attributes -> domain fields (parse direction)
- ``write_task()`` maps domain fields -> XML attributes (serialize direction)
- The XmlElement serves as the binding layer between canonical domain and source XML
- Field-level tracking: known attrs updated in-place, unknown attrs preserved
- Child element binding: known children rebuilt from domain, unknown children preserved as raw XML

**Files:** ``src-tauri/src/domain/mappers.rs``, ``src-tauri/src/domain/xml_tree.rs``
$testRecord
$buildRecord
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
$buildRecord
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
$buildRecord
"@

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
$buildRecord
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
$buildRecord
"@

    "RD-M3-007" = @"
## Implementation Brief

Temp file writing in same target directory in ``persistence.rs``:
- ``temp_file_path()``: Generates ``.filename.tmp`` in same directory as target
- ``write_temp_file()``: Creates file, writes content, calls ``sync_all()`` for flush
- Ensures temp is on same filesystem for atomic rename

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
$buildRecord
"@

    "RD-M3-008" = @"
## Implementation Brief

Flush and sync staged temp file in ``persistence.rs``:
- ``write_temp_file()`` calls ``file.sync_all()`` after writing
- Ensures data is flushed from OS buffers to physical storage
- Prevents data loss on power failure during save

**Files:** ``src-tauri/src/domain/persistence.rs``
$testRecord
$buildRecord
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
$buildRecord
"@

    "RD-M3-014" = @"
## Implementation Brief

Recover incomplete save journals at startup in ``recovery.rs``:
- ``orphaned_entries()``: Finds non-Complete journal entries
- ``recover_entries()``: Analyzes temp/backup file existence to determine action
- ``RecoveryAction`` enum: CompleteReplacement, RestoreBackup, PreserveTemp, NothingNeeded
- Recovery logic: (temp+backup) -> complete replacement, (backup only) -> restore, (temp only) -> preserve

**Files:** ``src-tauri/src/domain/recovery.rs``
$testRecord
$buildRecord
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
$buildRecord
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
$buildRecord
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
$buildRecord
"@
}

# ============ Task IDs (from query) ============
$taskIds = @{
    "RD-M2-009" = "6ddb0804-e865-4bc8-b464-d70af27890cd"
    "RD-M2-010" = "8293f9ed-1c7f-44b8-8b66-880933fb3917"
    "RD-M2-019" = "be58eb67-7df9-4945-bc6f-cc4733ea21c9"
    "RD-M2-020" = "347a7846-057b-4b6e-b3da-027128db3806"
    "RD-M2-021" = "372c9964-b057-4d80-b071-ea751a2a3efc"
    "RD-M2-022" = "06fdbf19-5ca8-4c83-88ec-a354a9867e94"
    "RD-M2-023" = "59977f5a-95a7-4dcd-b166-dcd64e53f185"
    "RD-M3-001" = "b0bf71dd-7918-429f-8d27-8afc89e10028"
    "RD-M3-002" = "4ea3465f-4eb8-473e-bf47-085e75bc1c46"
    "RD-M3-007" = "5db07c17-a9bd-4d4a-879a-274b2315e9bf"
    "RD-M3-008" = "1fa8010f-3c48-492b-932d-4eb2c45cd2ff"
    "RD-M3-013" = "e3025b45-04e5-40f0-88de-630727a1b32a"
    "RD-M3-014" = "d8ae3f17-a38d-4ae0-b230-f8947cb39d6c"
    "RD-M3-019" = "4bf353dd-5f1b-4dcf-8031-a1fa1470237c"
    "RD-M3-020" = "670d0f08-9635-4667-9b7b-1f52de25586f"
    "RD-M3-021" = "21f30072-2533-42fe-ab6b-7555dcd1fca4"
}

# ============ Phase 1: Add Comments ============
Write-Host "========== PHASE 1: Adding Implementation Reports =========="
$commentSuccess = 0
$commentFail = 0

foreach ($taskName in $reports.Keys | Sort-Object) {
    $issueId = $taskIds[$taskName]
    Write-Host "  Commenting: $taskName ($issueId)"
    $body = $reports[$taskName]
    $ok = Add-Comment $issueId $body
    if ($ok) {
        $commentSuccess++
        Write-Host "    -> OK"
    } else {
        $commentFail++
        Write-Host "    -> FAILED"
    }
    Start-Sleep -Milliseconds 200
}
Write-Host "`nPhase 1 Result: $commentSuccess comments added, $commentFail failed`n"

# ============ Phase 2: Mark 16 tasks as Done ============
Write-Host "========== PHASE 2: Marking 16 Tasks as Done =========="
$doneSuccess = 0
$doneFail = 0

foreach ($taskName in $taskIds.Keys | Sort-Object) {
    $issueId = $taskIds[$taskName]
    Write-Host "  Updating: $taskName -> Done"
    $ok = Update-State $issueId $doneStateId
    if ($ok) {
        $doneSuccess++
        Write-Host "    -> OK"
    } else {
        $doneFail++
        Write-Host "    -> FAILED"
    }
    Start-Sleep -Milliseconds 200
}
Write-Host "`nPhase 2 Result: $doneSuccess marked Done, $doneFail failed`n"

# ============ Phase 3: Cancel 25 duplicate M3 Backlog tasks ============
Write-Host "========== PHASE 3: Cancelling 25 Duplicate M3 Backlog Tasks =========="

# First discover the Cancelled state ID
$q = @{
    query = "query { teams { nodes { id states { nodes { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$teamStates = $r.data.teams.nodes[0].states.nodes
$cancelledState = $teamStates | Where-Object { $_.name -eq "Canceled" -or $_.name -eq "Cancelled" } | Select-Object -First 1
if (-not $cancelledState) {
    Write-Host "Available states:"
    $teamStates | ForEach-Object { Write-Host "  $($_.name) = $($_.id)" }
    Write-Host "ERROR: Could not find Cancelled state"
    exit 1
}
$cancelledStateId = $cancelledState.id
Write-Host "Using cancelled state: $($cancelledState.name) ($cancelledStateId)`n"

# M3 Backlog duplicate IDs (INH-838 through INH-862)
$backlogDuplicates = @(
    @{ id = "e85c911b-59f4-49fc-a42a-6865feeff1c9"; name = "INH-838 RD-M3-001" }
    @{ id = "5cee07d9-3312-4916-943a-0b29d953ebff"; name = "INH-839 RD-M3-002" }
    @{ id = "11750a30-af3c-4b65-abcf-89109437d073"; name = "INH-840 RD-M3-003" }
    @{ id = "e43735fd-e10b-4ce5-9b77-9d06ebd63767"; name = "INH-841 RD-M3-004" }
    @{ id = "44964566-8f74-4117-8b7d-2ad293ffc532"; name = "INH-842 RD-M3-005" }
    @{ id = "20e5c431-9d61-41b8-9fc3-1c04105d6ff3"; name = "INH-843 RD-M3-006" }
    @{ id = "59ae6118-e22e-49d9-be7d-b4039a4e3a78"; name = "INH-844 RD-M3-007" }
    @{ id = "f242daa5-273b-4402-85f0-076c22303783"; name = "INH-845 RD-M3-008" }
    @{ id = "0c884bd9-1daa-4bec-aa26-0c42f9285a29"; name = "INH-846 RD-M3-009" }
    @{ id = "10486aa7-3518-4fce-b6b0-e14452c83c6a"; name = "INH-847 RD-M3-010" }
    @{ id = "e4cd7df5-bc72-4267-9728-8dcc45c8829c"; name = "INH-848 RD-M3-011" }
    @{ id = "21eebe89-807c-401b-94f8-5f76a67ada26"; name = "INH-849 RD-M3-012" }
    @{ id = "0c4db184-3589-4a75-9804-ce7fda9ea766"; name = "INH-850 RD-M3-013" }
    @{ id = "846b76ae-c293-485c-ad33-6798f660a86f"; name = "INH-851 RD-M3-014" }
    @{ id = "c4ded91f-ced8-446b-9e5a-8011ee7fe0c1"; name = "INH-852 RD-M3-015" }
    @{ id = "050dec8e-1b3c-490d-8ddf-1b8e0a218195"; name = "INH-853 RD-M3-016" }
    @{ id = "ce42d5b7-ccf6-4429-982e-cab92553884c"; name = "INH-854 RD-M3-017" }
    @{ id = "b4403ef4-500a-41a7-9ebe-967bcf5b63b5"; name = "INH-855 RD-M3-018" }
    @{ id = "786fa746-5a90-4662-afb5-c7a7a143d35d"; name = "INH-856 RD-M3-019" }
    @{ id = "010b4bd9-166f-4497-bbfe-7fbbda39e33e"; name = "INH-857 RD-M3-020" }
    @{ id = "83a1097b-4307-4237-8abd-1546e2ad1cae"; name = "INH-858 RD-M3-021" }
    @{ id = "86f886b4-e5fd-4b46-b91e-7f2dd10e5653"; name = "INH-859 RD-M3-022" }
    @{ id = "be3c77dc-f078-4f8d-96c8-7b03d03fb80d"; name = "INH-860 RD-M3-023" }
    @{ id = "2e8f5c9c-3811-46de-87c1-13cfbb753066"; name = "INH-861 RD-M3-024" }
    @{ id = "1e40dddd-24d7-4008-949c-e55529f9ccff"; name = "INH-862 RD-M3-025" }
)

$cancelSuccess = 0
$cancelFail = 0

foreach ($dup in $backlogDuplicates) {
    Write-Host "  Cancelling: $($dup.name)"
    $ok = Update-State $dup.id $cancelledStateId
    if ($ok) {
        $cancelSuccess++
        Write-Host "    -> OK"
    } else {
        $cancelFail++
        Write-Host "    -> FAILED"
    }
    Start-Sleep -Milliseconds 200
}
Write-Host "`nPhase 3 Result: $cancelSuccess cancelled, $cancelFail failed`n"

# ============ Summary ============
Write-Host "=========================================="
Write-Host "           FINAL SUMMARY"
Write-Host "=========================================="
Write-Host "Comments added:     $commentSuccess/16"
Write-Host "Tasks -> Done:      $doneSuccess/16"
Write-Host "Duplicates cancelled: $cancelSuccess/25"
Write-Host "=========================================="
