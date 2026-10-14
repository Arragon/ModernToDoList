$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

function Update-ToDone($issueId, $identifier) {
    $body = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $issueId; stateId = $doneStateId }
    } | ConvertTo-Json -Depth 5 -Compress
    try {
        $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body -ErrorAction Stop
        if ($r.data.issueUpdate.success) { Write-Host "OK: $identifier -> Done"; return $true }
    } catch { Write-Host "FAIL: $identifier - $_" }
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

# Find M2/M3 QA + GATE + Epic tasks still in Backlog
$targets = $allIssues | Where-Object {
    $_.state.name -eq "Backlog" -and (
        $_.title -match "^QA-M2-" -or
        $_.title -match "^QA-M3-" -or
        $_.title -match "^GATE-M2" -or
        $_.title -match "^GATE-M3" -or
        $_.title -match "^M2 Epic" -or
        $_.title -match "^M3 Epic"
    )
}

Write-Host "Found $($targets.Count) M2/M3 QA+GATE+Epic tasks to sync"
$updated = 0
foreach ($t in $targets) {
    if (Update-ToDone $t.id $t.identifier) { $updated++ }
    Start-Sleep -Milliseconds 150
}
Write-Host "`nSynced $updated/$($targets.Count) tasks to Done"
