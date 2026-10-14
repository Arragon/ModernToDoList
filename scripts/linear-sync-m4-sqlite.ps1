$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

function Add-CommentAndDone($issueId, $identifier, $comment) {
    # Add comment
    $commentBody = @{
        query = 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }'
        variables = @{ issueId = $issueId; body = $comment }
    } | ConvertTo-Json -Depth 5 -Compress
    try {
        Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $commentBody -ErrorAction Stop | Out-Null
    } catch { Write-Host "Comment failed for $identifier" }
    Start-Sleep -Milliseconds 100

    # Mark Done
    $doneBody = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success } }'
        variables = @{ issueId = $issueId; stateId = $doneStateId }
    } | ConvertTo-Json -Depth 5 -Compress
    try {
        $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $doneBody -ErrorAction Stop
        if ($r.data.issueUpdate.success) { Write-Host "OK: $identifier -> Done"; return $true }
    } catch { Write-Host "FAIL: $identifier" }
    return $false
}

# Fetch all issues
$allIssues = @()
$after = $null
do {
    $cursorPart = if ($after) { ", after: `"$after`"" } else { "" }
    $body = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100$cursorPart) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }"
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
    $allIssues += $r.data.project.issues.nodes
    $hasNext = $r.data.project.issues.pageInfo.hasNextPage
    $after = $r.data.project.issues.pageInfo.endCursor
} while ($hasNext)

$m4SqliteReport = @"
## M4 SQLite Infrastructure - Implementation Report

**RD-M4-001~018 completed:**

- Added rusqlite v0.32 with bundled feature (no system SQLite dependency)
- Added uuid v1 for stable ID generation
- Created DatabaseManager: opens Data/index.db with WAL mode, fallback to DELETE for UNC paths
- Created MigrationRunner: versioned schema migrations with transactional apply
- Created full schema with 12 tables: schema_migrations, workspaces, documents, task_index, task_tags, task_participants, task_dependencies, attachments_index, progress_links_index, saved_views, ui_state, recovery_records
- Created 5 indexes: task parent, task document, tags, participants, dependencies
- Foreign keys enabled, busy_timeout = 5s
- RecoveryMode: if DB fails to open, XML remains accessible
- 18 new unit tests: schema versioning, migration idempotency, table/index creation, cascade delete, future schema rejection, WAL mode, delete+reopen, clone sharing

**Test results:** 190 total tests passing (172 unit + 17 integration + 1 doctest)
"@

$m4SqliteTasks = $allIssues | Where-Object {
    $_.state.name -eq "Backlog" -and $_.title -match "^RD-M4-00[0-9]|^RD-M4-01[0-8]"
}

Write-Host "=== Syncing M4 SQLite tasks ($($m4SqliteTasks.Count) tasks) ==="
$updated = 0
foreach ($t in $m4SqliteTasks) {
    if (Add-CommentAndDone $t.id $t.identifier $m4SqliteReport) { $updated++ }
    Start-Sleep -Milliseconds 200
}
Write-Host "Synced $updated/$($m4SqliteTasks.Count) M4 SQLite tasks"
