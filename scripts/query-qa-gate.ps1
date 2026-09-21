$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Query with title filter for QA-M2
Write-Host "=== Searching for QA-M2 tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"QA-M2`" } }) { nodes { id identifier title state { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m2qa = $r.data.project.issues.nodes
Write-Host "QA-M2 tasks found: $($m2qa.Count)"
$m2qa | ForEach-Object { Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title)" }

# Query with title filter for GATE-M2
Write-Host ""
Write-Host "=== Searching for GATE-M2 tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"GATE-M2`" } }) { nodes { id identifier title state { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m2gate = $r.data.project.issues.nodes
Write-Host "GATE-M2 tasks found: $($m2gate.Count)"
$m2gate | ForEach-Object { Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title)" }

# Query with title filter for QA-M3
Write-Host ""
Write-Host "=== Searching for QA-M3 tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"QA-M3`" } }) { nodes { id identifier title state { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m3qa = $r.data.project.issues.nodes
Write-Host "QA-M3 tasks found: $($m3qa.Count)"
$m3qa | ForEach-Object { Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title)" }

# Query with title filter for GATE-M3
Write-Host ""
Write-Host "=== Searching for GATE-M3 tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"GATE-M3`" } }) { nodes { id identifier title state { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m3gate = $r.data.project.issues.nodes
Write-Host "GATE-M3 tasks found: $($m3gate.Count)"
$m3gate | ForEach-Object { Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title)" }

# Now check overall M2 and M3 counts
Write-Host ""
Write-Host "=== Overall M2 status ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"RD-M2`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m2rd = $r.data.project.issues.nodes
$m2done = ($m2rd | Where-Object { $_.state.name -eq "Done" }).Count
$m2bl = ($m2rd | Where-Object { $_.state.name -eq "Backlog" }).Count
$m2c = ($m2rd | Where-Object { $_.state.name -eq "Canceled" }).Count
Write-Host "RD-M2: Done=$m2done, Backlog=$m2bl, Canceled=$m2c, Total=$($m2rd.Count)"

Write-Host ""
Write-Host "=== Overall M3 status ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"RD-M3`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m3rd = $r.data.project.issues.nodes
$m3done = ($m3rd | Where-Object { $_.state.name -eq "Done" }).Count
$m3bl = ($m3rd | Where-Object { $_.state.name -eq "Backlog" }).Count
$m3c = ($m3rd | Where-Object { $_.state.name -eq "Canceled" }).Count
Write-Host "RD-M3: Done=$m3done, Backlog=$m3bl, Canceled=$m3c, Total=$($m3rd.Count)"

# Check total issue count
Write-Host ""
Write-Host "=== Total project issues ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 1) { totalCount } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
Write-Host "Total issues: $($r.data.project.issues.totalCount)"
