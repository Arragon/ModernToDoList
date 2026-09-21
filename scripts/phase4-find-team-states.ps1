$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

# Get the team for INH-838 (which we just set to Done, so let's use another backlog task)
# Actually INH-838 is now Done. Let's query INH-839 which is still Backlog
$testId = "5cee07d9-3312-4916-943a-0b29d953ebff"  # INH-839

$q = @{
    query = "query { issue(id: `"$testId`") { id identifier title state { id name } team { id name states { nodes { id name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$issue = $r.data.issue
Write-Host "Issue: $($issue.identifier)"
Write-Host "Team: $($issue.team.name) ($($issue.team.id))"
Write-Host "Current state: $($issue.state.name) ($($issue.state.id))"
Write-Host "`nTeam states:"
$issue.team.states.nodes | ForEach-Object {
    Write-Host "  $($_.name) = $($_.id)"
}

# Also fix INH-838 - it was wrongly set to Done, should be Cancelled
# Use the correct cancelled state from this team
$correctCancelled = $issue.team.states.nodes | Where-Object { $_.name -match "Cancel" } | Select-Object -First 1
if ($correctCancelled) {
    Write-Host "`nCorrect Cancelled state: $($correctCancelled.name) ($($correctCancelled.id))"
    
    # Fix INH-838 (was set to Done by mistake)
    $inh838 = "e85c911b-59f4-49fc-a42a-6865feeff1c9"
    $payload = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $inh838; stateId = $correctCancelled.id }
    } | ConvertTo-Json -Depth 5 -Compress
    $r2 = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    Write-Host "`nFix INH-838: $($r2.data.issueUpdate.success) -> $($r2.data.issueUpdate.issue.state.name)"
}
