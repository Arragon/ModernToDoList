$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

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

# M1 Backlog tasks
Write-Host "=== M1 Backlog Tasks ==="
$m1Backlog = $allIssues | Where-Object { $_.title -match "M1" -and $_.state.name -eq "Backlog" } | Sort-Object identifier
foreach ($t in $m1Backlog) {
    Write-Host "  $($t.identifier) $($t.title)"
}

# M2/M3 Backlog tasks
Write-Host ""
Write-Host "=== M2/M3 Backlog Tasks (not yet synced?) ==="
$m2m3Backlog = $allIssues | Where-Object { ($_.title -match "M2" -or $_.title -match "M3") -and $_.state.name -eq "Backlog" } | Sort-Object identifier
foreach ($t in $m2m3Backlog) {
    Write-Host "  $($t.identifier) $($t.title)"
}

# Check code state
Write-Host ""
Write-Host "=== Git Status ==="
$gitLog = git -C "d:\Project\ModernToDoList" log --oneline --all
Write-Host $gitLog
