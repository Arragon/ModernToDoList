$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Query all project issues
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress

$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$nodes = $r.data.project.issues.nodes

Write-Host "Total issues in project: $($nodes.Count)`n"

# Group by state
$states = $nodes | Group-Object { $_.state.name }
foreach ($g in $states) {
    Write-Host "=== $($g.Name) ($($g.Count)) ==="
    $g.Group | Sort-Object identifier | ForEach-Object {
        $short = $_.title
        if ($short.Length -gt 70) { $short = $short.Substring(0, 70) + "..." }
        Write-Host "  $($_.identifier) | $short"
    }
    Write-Host ""
}

# Specifically look for the 16 tasks we need
Write-Host "`n=== Target 16 Tasks ==="
$targets = @("RD-M2-009","RD-M2-010","RD-M2-019","RD-M2-020","RD-M2-021","RD-M2-022","RD-M2-023",
             "RD-M3-001","RD-M3-002","RD-M3-007","RD-M3-008","RD-M3-013","RD-M3-014",
             "RD-M3-019","RD-M3-020","RD-M3-021")

foreach ($t in $targets) {
    $found = $nodes | Where-Object { $_.title -match "^$t" } | Select-Object -First 1
    if ($found) {
        Write-Host "  $t -> $($found.state.name) | $($found.identifier) | $($found.id)"
    } else {
        Write-Host "  $t -> NOT FOUND"
    }
}

# Look for M3 Backlog duplicates
Write-Host "`n=== M3 Backlog Tasks (potential duplicates) ==="
$m3Backlog = $nodes | Where-Object { $_.title -match "^RD-M3-" -and $_.state.name -eq "Backlog" }
Write-Host "M3 Backlog count: $($m3Backlog.Count)"
$m3Backlog | Sort-Object identifier | ForEach-Object {
    Write-Host "  $($_.identifier) | $($_.id) | $($_.title.Substring(0, [Math]::Min(60, $_.title.Length)))"
}
