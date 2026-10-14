$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

# Test with one task - show full error
$testId = "e85c911b-59f4-49fc-a42a-6865feeff1c9"  # INH-838
$cancelledStateId = "3348ad3c-49ad-4edb-bb17-2eaa02b9208b"

# First verify the issue exists and get its current state
$q = @{
    query = "query { issue(id: `"$testId`") { id identifier title state { id name } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
Write-Host "Issue: $($r.data.issue.identifier) - $($r.data.issue.state.name) ($($r.data.issue.state.id))"

# Try updating with the discovered cancelled state
$payload = @{
    query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
    variables = @{ issueId = $testId; stateId = $cancelledStateId }
} | ConvertTo-Json -Depth 5 -Compress

Write-Host "`nPayload: $payload"
$r2 = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
Write-Host "`nResult: $($r2 | ConvertTo-Json -Depth 5)"

# Also try the done state ID to see if that works
Write-Host "`n--- Trying Done state ---"
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$payload2 = @{
    query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
    variables = @{ issueId = $testId; stateId = $doneStateId }
} | ConvertTo-Json -Depth 5 -Compress

$r3 = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload2
Write-Host "Result: $($r3 | ConvertTo-Json -Depth 5)"
