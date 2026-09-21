$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"
$allNodes = @()
$after = $null

do {
    $afterArg = if ($after) { ", after: `"$after`"" } else { "" }
    $q = @{ query = "query { project(id: `"$projectId`") { issues(first: 100$afterArg) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }" } | ConvertTo-Json -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
    $nodes = $r.data.project.issues.nodes
    $allNodes += $nodes
    $hasNext = $r.data.project.issues.pageInfo.hasNextPage
    $after = $r.data.project.issues.pageInfo.endCursor
    Write-Host "Page: $($nodes.Count) nodes, total so far: $($allNodes.Count), hasNext: $hasNext"
} while ($hasNext)

Write-Host "`nTotal tasks found: $($allNodes.Count)"

$allNodes | ForEach-Object { "$($_.identifier)|$($_.state.name)|$($_.title)" } | Out-File -FilePath "d:\Project\ModernToDoList\scripts\task-list.txt" -Encoding utf8

Write-Host "Saved to task-list.txt"
