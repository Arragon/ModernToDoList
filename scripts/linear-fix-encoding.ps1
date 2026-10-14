# Fix v3: Read comments from UTF-8 JSON file, send via Linear API
# Root cause of previous failures: PowerShell reads .ps1 files with system encoding (GBK on Chinese Windows),
# corrupting Chinese characters in here-strings. JSON file read with explicit UTF-8 avoids this entirely.

$authHeader = '$env:LINEAR_API_KEY'
$apiHeaders = @{
    'Authorization' = $authHeader
    'Content-Type' = 'application/json; charset=utf-8'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$commentsFile = Join-Path $scriptDir 'linear-comments.json'

# Read comments JSON with explicit UTF-8 encoding
$commentsJson = [System.IO.File]::ReadAllText($commentsFile, [System.Text.Encoding]::UTF8)
$comments = $commentsJson | ConvertFrom-Json

function Invoke-Linear($jsonObj) {
    $json = $jsonObj | ConvertTo-Json -Depth 5 -Compress
    $utf8 = [System.Text.Encoding]::UTF8.GetBytes($json)
    $r = Invoke-WebRequest -Uri 'https://api.linear.app/graphql' -Method Post `
        -Headers $apiHeaders -Body $utf8 -UseBasicParsing -ErrorAction Stop
    return ($r.Content | ConvertFrom-Json)
}

function Get-CommentForTask($title) {
    switch -Regex ($title) {
        "^RD-M4-0(19|2[0-9]|3[0-9])" { return $comments.workspace_domain }
        "^RD-M4-0(3[1-9]|4[0-6])"   { return $comments.file_watcher }
        "^QA-M4-"                     { return $comments.qa_m4 }
        "^GATE-M4"                    { return $comments.gate_m4 }
        "^RD-M5-00[1-9]|^RD-M5-01[0-4]" { return $comments.ui_infra }
        "^RD-M5-01[5-9]|^RD-M5-02[0-9]|^RD-M5-030" { return $comments.task_tree }
        "^RD-M5-03[1-9]|^RD-M5-040"  { return $comments.inspector }
        "^RD-M5-04[1-9]|^RD-M5-05[0-2]" { return $comments.keyboard }
        "^RD-M5-05[3-9]|^RD-M5-06[0-4]" { return $comments.filter }
        "^QA-M5-"                     { return $comments.qa_m5 }
        "^GATE-M5"                    { return $comments.gate_m5 }
        default                       { return $comments.ui_infra }
    }
}

# Step 1: Fetch all Done issues
$allIssues = @()
$after = $null
do {
    $cursorPart = if ($after) { ", after: `"$after`"" } else { "" }
    $r = Invoke-Linear @{
        query = "query { project(id: `"$projectId`") { issues(first: 100$cursorPart) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }"
    }
    $allIssues += $r.data.project.issues.nodes
    $hasNext = $r.data.project.issues.pageInfo.hasNextPage
    $after = $r.data.project.issues.pageInfo.endCursor
} while ($hasNext)

$doneIssues = $allIssues | Where-Object {
    ($_.title -match "^RD-M[45]-|^QA-M[45]-|^GATE-M[45]|^M[45] Epic") -and
    $_.state.name -eq "Done"
}
Write-Host "=== Found $($doneIssues.Count) Done tasks ==="

# Step 2: For each issue, delete garbled comments and send correct ones
$updated = 0
$failed = 0

foreach ($t in ($doneIssues | Sort-Object { $_.identifier })) {
    Write-Host "`nProcessing $($t.identifier): $($t.title)"

    # List existing comments
    $listResult = Invoke-Linear @{
        query = 'query($issueId: String!) { issue(id: $issueId) { comments { nodes { id body createdAt } } } }'
        variables = @{ issueId = $t.id }
    }
    $existingComments = $listResult.data.issue.comments.nodes

    # Delete garbled comments: match the exact corrupted header pattern
    # Garbled form of "##  进度更新" or "##  进度更新" becomes "## ?? ??????" or mojibake starting with "## "
    # We detect by checking if the body contains the garbled section headers
    $deletedCount = 0
    foreach ($c in $existingComments) {
        $isGarbled = $false
        # Pattern 1: ?? question marks (first sync failure)
        if ($c.body -match '^## \?\? \?\?\?\?\?\?') { $isGarbled = $true }
        # Pattern 2: Mojibake - header contains non-ASCII garbage that is NOT valid Chinese
        # Valid headers start with "## " followed by emoji or Chinese "进度更新"
        # Garbled headers have random CJK mojibake characters
        elseif ($c.body -match '^## [^\x{4e00}-\x{9fff}\x{1f300}-\x{1f9ff}\s]' -and $c.body -notmatch '## 📋' -and $c.body -notmatch '##  进度') {
            # Check if it looks like our progress update template (has the section structure)
            if ($c.body -match '实现方案|遇到的问题|解决方案|潜在风险|测试方案') {
                # It has our section structure but the header is garbled - it's one of our bad comments
                # But we need to be careful: a VALID comment also has these sections
                # Only delete if the H2 header is NOT "##  进度更新" or "##  进度更新"
                if ($c.body -notmatch '## [📋]?\s*进度更新') {
                    $isGarbled = $true
                }
            }
        }

        if ($isGarbled) {
            $delResult = Invoke-Linear @{
                query = 'mutation($id: String!) { commentDelete(id: $id) { success } }'
                variables = @{ id = $c.id }
            }
            if ($delResult.data.commentDelete.success) {
                Write-Host "  Deleted garbled comment"
                $deletedCount++
            }
            Start-Sleep -Milliseconds 200
        }
    }

    # Send correct comment from JSON file
    $comment = Get-CommentForTask $t.title
    $addResult = Invoke-Linear @{
        query = 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }'
        variables = @{ issueId = $t.id; body = $comment }
    }
    if ($addResult.data.commentCreate.success) {
        Write-Host "  Sent correct comment ($deletedCount garbled deleted)"
        $updated++
    } else {
        Write-Host "  FAIL: comment create failed"
        $failed++
    }
    Start-Sleep -Milliseconds 200
}

Write-Host "`n=== Fix complete: $updated success, $failed failed out of $($doneIssues.Count) tasks ==="
