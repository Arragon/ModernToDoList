$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Query M2 tasks specifically
Write-Host "=== M2 Tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 50, filter: { title: { contains: `"RD-M2`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m2nodes = $r.data.project.issues.nodes
Write-Host "M2 tasks found: $($m2nodes.Count)"
$m2nodes | Sort-Object identifier | ForEach-Object {
    Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title.Substring(0, [Math]::Min(60, $_.title.Length)))"
}

# Query M3 tasks specifically
Write-Host "`n=== M3 Tasks ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"RD-M3`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m3nodes = $r.data.project.issues.nodes
Write-Host "M3 tasks found: $($m3nodes.Count)"
$m3nodes | Sort-Object identifier | ForEach-Object {
    Write-Host "  $($_.identifier) | $($_.state.name) | $($_.id) | $($_.title.Substring(0, [Math]::Min(60, $_.title.Length)))"
}

# Also check total project issue count with pagination
Write-Host "`n=== Total project issues (counting) ==="
$q = @{
    query = "query { project(id: `"$projectId`") { issues { totalCount nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
Write-Host "Total count: $($r.data.project.issues.totalCount)"
$allNodes = $r.data.project.issues.nodes
Write-Host "Nodes returned: $($allNodes.Count)"

# Check if any M2/M3 tasks exist in all nodes
$m2inAll = $allNodes | Where-Object { $_.title -match "RD-M2" }
$m3inAll = $allNodes | Where-Object { $_.title -match "RD-M3" }
Write-Host "M2 in all nodes: $($m2inAll.Count)"
Write-Host "M3 in all nodes: $($m3inAll.Count)"
