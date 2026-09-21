$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"

# Test with a single issue first
$testIssueId = "719"  # Will get full ID from query

# Query to get issue IDs
$query = @'
{
  "query": "query { project(id: \"72e576a7-e894-4e22-b52c-5d47d43a5912\") { issues(first: 50, filter: { title: { contains: \"RD-M0\" } }) { nodes { id identifier title state { name } } } } }"
}
'@

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
$issues = $response.data.project.issues.nodes | Where-Object { $_.state.name -ne "Canceled" }

Write-Host "Found $($issues.Count) RD-M0 issues"

if ($issues.Count -gt 0) {
    # Test update with first issue
    $firstIssue = $issues[0]
    Write-Host "Testing with: $($firstIssue.identifier) (ID: $($firstIssue.id))"
    
    # Build proper JSON body
    $body = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{
            issueId = $firstIssue.id
            stateId = $doneStateId
        }
    } | ConvertTo-Json -Depth 5 -Compress
    
    Write-Host "Request body: $body"
    
    try {
        $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body -ErrorAction Stop
        Write-Host "Result: $($result | ConvertTo-Json -Depth 5)"
    } catch {
        Write-Host "Error: $($_.Exception.Message)"
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        $reader.BaseStream.Position = 0
        $responseBody = $reader.ReadToEnd()
        Write-Host "Response: $responseBody"
    }
}
