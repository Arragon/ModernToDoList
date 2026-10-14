$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Fetch all issues with pagination
$allIssues = @()
$after = $null
do {
    $cursorPart = if ($after) { ", after: `"$after`"" } else { "" }
    $body = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100$cursorPart) { nodes { identifier title state { name } } pageInfo { hasNextPage endCursor } } } }"
    } | ConvertTo-Json -Depth 5 -Compress
    
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
    $allIssues += $r.data.project.issues.nodes
    $hasNext = $r.data.project.issues.pageInfo.hasNextPage
    $after = $r.data.project.issues.pageInfo.endCursor
} while ($hasNext)

Write-Host "Total issues: $($allIssues.Count)"
Write-Host ""

# Group by state
$grouped = $allIssues | Group-Object { $_.state.name } | Sort-Object Name
Write-Host "=== By State ==="
foreach ($g in $grouped) {
    Write-Host "  $($g.Name): $($g.Count)"
}
Write-Host ""

# Milestone breakdown
Write-Host "=== Milestone Breakdown ==="
$milestones = @("M0","M1","M2","M3","M4","M5","M6","M7","M8","M9","M10")
foreach ($m in $milestones) {
    $mIssues = $allIssues | Where-Object { $_.identifier -match $m -and $_.identifier -notmatch "M1[0-9]" -or ($m -eq "M1" -and $_.identifier -match "M1[^0-9]") -or ($m -eq "M10" -and $_.identifier -match "M10") }
    # Simpler approach: match the milestone tag in title
    $mIssues = $allIssues | Where-Object { $_.title -match $m }
    if ($mIssues.Count -gt 0) {
        $done = ($mIssues | Where-Object { $_.state.name -eq "Done" }).Count
        $canceled = ($mIssues | Where-Object { $_.state.name -eq "Canceled" }).Count
        $backlog = ($mIssues | Where-Object { $_.state.name -eq "Backlog" }).Count
        $other = $mIssues.Count - $done - $canceled - $backlog
        Write-Host "  $m`: $($mIssues.Count) total | Done:$done | Canceled:$canceled | Backlog:$backlog | Other:$other"
    }
}

Write-Host ""
Write-Host "=== Done Tasks ==="
$doneIssues = $allIssues | Where-Object { $_.state.name -eq "Done" } | Sort-Object identifier
foreach ($t in $doneIssues) {
    Write-Host "  $($t.identifier) $($t.title)"
}
