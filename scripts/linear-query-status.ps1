$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

$body = @{
    query = "query { project(id: `"$projectId`") { issues(first: 250) { nodes { identifier title state { name } } } } }"
} | ConvertTo-Json -Depth 5 -Compress

$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
$issues = $r.data.project.issues.nodes | Sort-Object identifier

# Group by state
$grouped = $issues | Group-Object { $_.state.name } | Sort-Object Name
Write-Host "=== Linear Project Status Summary ==="
foreach ($g in $grouped) {
    Write-Host "$($g.Name): $($g.Count) tasks"
}
Write-Host ""
Write-Host "Total: $($issues.Count) tasks"
Write-Host ""

# Show non-Done, non-Canceled tasks
$active = $issues | Where-Object { $_.state.name -ne "Done" -and $_.state.name -ne "Canceled" }
if ($active.Count -gt 0) {
    Write-Host "=== Active/In-Progress Tasks ==="
    foreach ($t in $active) {
        Write-Host "  $($t.identifier) [$($t.state.name)] $($t.title)"
    }
} else {
    Write-Host "=== All tasks are Done or Canceled ==="
}

# Show milestones breakdown
Write-Host ""
Write-Host "=== Milestone Breakdown ==="
$milestones = @("M0", "M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9")
foreach ($m in $milestones) {
    $mIssues = $issues | Where-Object { $_.identifier -match $m }
    if ($mIssues.Count -gt 0) {
        $done = ($mIssues | Where-Object { $_.state.name -eq "Done" }).Count
        $canceled = ($mIssues | Where-Object { $_.state.name -eq "Canceled" }).Count
        $active = ($mIssues | Where-Object { $_.state.name -ne "Done" -and $_.state.name -ne "Canceled" }).Count
        Write-Host "$m`: $($mIssues.Count) total | $done Done | $canceled Canceled | $active Active"
    }
}
