$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Query to get RD-M2-001 issue ID
$query = @{
    query = "query { project(id: `"$projectId`") { issues(first: 10, filter: { title: { contains: `"RD-M2-001`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
$issue = $response.data.project.issues.nodes | Where-Object { $_.title -match "^RD-M2-001" } | Select-Object -First 1

if ($issue) {
    Write-Host "Found: $($issue.identifier) - $($issue.title)"
    Write-Host "Current state: $($issue.state.name)"
    
    # Update to Done
    $body = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{
            issueId = $issue.id
            stateId = $doneStateId
        }
    } | ConvertTo-Json -Depth 5 -Compress
    
    $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
    if ($result.data.issueUpdate.success) {
        Write-Host "Updated: $($issue.identifier) -> Done"
    }
}
