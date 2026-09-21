$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$inProgressStateId = "5c23a178-e79b-403f-9555-2164e9ce4005"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

$taskIds = @(
    "RD-M2-009", "RD-M2-010", "RD-M2-019", "RD-M2-020", "RD-M2-021", "RD-M2-022", "RD-M2-023",
    "RD-M3-001", "RD-M3-002", "RD-M3-007", "RD-M3-008", "RD-M3-013", "RD-M3-014",
    "RD-M3-019", "RD-M3-020", "RD-M3-021"
)

$updated = 0
$failed = 0

foreach ($taskRef in $taskIds) {
    Write-Host "`n--- Processing $taskRef ---"
    
    # Query for the issue
    $query = @{
        query = "query { project(id: `"$projectId`") { issues(first: 5, filter: { title: { contains: `"$taskRef`" } }) { nodes { id identifier title state { name id } } } } }"
    } | ConvertTo-Json -Compress

    try {
        $response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
        $issue = $response.data.project.issues.nodes | Where-Object { $_.title -match "^$taskRef" } | Select-Object -First 1

        if (-not $issue) {
            Write-Host "WARNING: Could not find issue matching $taskRef" -ForegroundColor Yellow
            $failed++
            continue
        }

        Write-Host "Found: $($issue.identifier) - $($issue.title) [Current: $($issue.state.name)]"

        # Skip if already In Progress
        if ($issue.state.name -eq "In Progress") {
            Write-Host "Already In Progress, skipping."
            $updated++
            continue
        }

        # Update to In Progress
        $body = @{
            query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
            variables = @{ issueId = $issue.id; stateId = $inProgressStateId }
        } | ConvertTo-Json -Depth 5 -Compress

        $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
        if ($result.data.issueUpdate.success) {
            Write-Host "Updated: $($issue.identifier) -> In Progress" -ForegroundColor Green
            $updated++
        } else {
            Write-Host "FAILED to update $($issue.identifier)" -ForegroundColor Red
            $failed++
        }
    } catch {
        Write-Host "ERROR processing $taskRef : $_" -ForegroundColor Red
        $failed++
    }
}

Write-Host "`n=============================="
Write-Host "Total tasks: $($taskIds.Count)"
Write-Host "Updated: $updated"
Write-Host "Failed: $failed"
Write-Host "=============================="
