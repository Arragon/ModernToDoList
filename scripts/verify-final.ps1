$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Count all M2-related tasks (RD + QA + GATE)
$categories = @(
    @{ name = "RD-M2"; filter = "RD-M2" }
    @{ name = "QA-M2"; filter = "QA-M2" }
    @{ name = "GATE-M2"; filter = "GATE-M2" }
    @{ name = "RD-M3"; filter = "RD-M3" }
    @{ name = "QA-M3"; filter = "QA-M3" }
    @{ name = "GATE-M3"; filter = "GATE-M3" }
)

foreach ($cat in $categories) {
    $q = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"$($cat.filter)`" } }) { nodes { id identifier title state { name } } } } }"
    } | ConvertTo-Json -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
    $nodes = $r.data.project.issues.nodes
    
    $done = ($nodes | Where-Object { $_.state.name -eq "Done" }).Count
    $bl = ($nodes | Where-Object { $_.state.name -eq "Backlog" }).Count
    $ip = ($nodes | Where-Object { $_.state.name -eq "In Progress" }).Count
    $c = ($nodes | Where-Object { $_.state.name -eq "Canceled" }).Count
    
    Write-Host "$($cat.name): Total=$($nodes.Count) | Done=$done | Backlog=$bl | InProgress=$ip | Canceled=$c"
}

Write-Host ""
Write-Host "=== M2 Combined ==="
$m2total = 0; $m2done = 0; $m2bl = 0; $m2c = 0
foreach ($cat in @("RD-M2", "QA-M2", "GATE-M2")) {
    $q = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"$cat`" } }) { nodes { state { name } } } } }"
    } | ConvertTo-Json -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
    $nodes = $r.data.project.issues.nodes
    $m2total += $nodes.Count
    $m2done += ($nodes | Where-Object { $_.state.name -eq "Done" }).Count
    $m2bl += ($nodes | Where-Object { $_.state.name -eq "Backlog" }).Count
    $m2c += ($nodes | Where-Object { $_.state.name -eq "Canceled" }).Count
}
Write-Host "M2 Total: $m2total | Done: $m2done | Backlog: $m2bl | Canceled: $m2c"
$pct = if ($m2total -gt 0) { [math]::Round(($m2done / $m2total) * 100) } else { 0 }
Write-Host "M2 Progress: $pct%"

Write-Host ""
Write-Host "=== M3 Combined ==="
$m3total = 0; $m3done = 0; $m3bl = 0; $m3c = 0
foreach ($cat in @("RD-M3", "QA-M3", "GATE-M3")) {
    $q = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"$cat`" } }) { nodes { state { name } } } } }"
    } | ConvertTo-Json -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
    $nodes = $r.data.project.issues.nodes
    $m3total += $nodes.Count
    $m3done += ($nodes | Where-Object { $_.state.name -eq "Done" }).Count
    $m3bl += ($nodes | Where-Object { $_.state.name -eq "Backlog" }).Count
    $m3c += ($nodes | Where-Object { $_.state.name -eq "Canceled" }).Count
}
Write-Host "M3 Total: $m3total | Done: $m3done | Backlog: $m3bl | Canceled: $m3c"
$pct = if ($m3total -gt 0) { [math]::Round(($m3done / $m3total) * 100) } else { 0 }
Write-Host "M3 Progress: $pct%"
