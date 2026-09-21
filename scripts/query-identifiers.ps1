$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"
$q = @{ query = "query { project(id: `"$projectId`") { issues(first: 200) { nodes { id identifier title state { name } } } } }" } | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$r.data.project.issues.nodes | ForEach-Object { Write-Host "$($_.identifier) | $($_.state.name) | $($_.title)" }
