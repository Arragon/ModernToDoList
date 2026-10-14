$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$inProgressStateId = "5c23a178-e79b-403f-9555-2164e9ce4005"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Update RD-M2-002 to In Progress
$query = @{
    query = "query { project(id: `"$projectId`") { issues(first: 10, filter: { title: { contains: `"RD-M2-002`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
$issue = $response.data.project.issues.nodes | Where-Object { $_.title -match "^RD-M2-002" } | Select-Object -First 1

if ($issue) {
    Write-Host "Found: $($issue.identifier) - $($issue.title)"
    $body = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $issue.id; stateId = $inProgressStateId }
    } | ConvertTo-Json -Depth 5 -Compress
    
    $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
    if ($result.data.issueUpdate.success) { Write-Host "Updated: $($issue.identifier) -> In Progress" }
}
