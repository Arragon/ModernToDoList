$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# M2 tasks to mark as done
$m2Tasks = @(
    "RD-M2-005", "RD-M2-006", "RD-M2-007", "RD-M2-008", "RD-M2-009", "RD-M2-010",
    "RD-M2-011", "RD-M2-012", "RD-M2-013", "RD-M2-014", "RD-M2-015", "RD-M2-016",
    "RD-M2-017", "RD-M2-018", "RD-M2-019", "RD-M2-020", "RD-M2-021", "RD-M2-022",
    "RD-M2-023", "RD-M2-024", "RD-M2-025", "RD-M2-026", "RD-M2-027", "RD-M2-028",
    "RD-M2-029", "RD-M2-030"
)

# M3 tasks to mark as done
$m3Tasks = @(
    "RD-M3-001", "RD-M3-002", "RD-M3-003", "RD-M3-004", "RD-M3-005", "RD-M3-006",
    "RD-M3-007", "RD-M3-008", "RD-M3-009", "RD-M3-010", "RD-M3-011", "RD-M3-012",
    "RD-M3-013", "RD-M3-014", "RD-M3-015", "RD-M3-016", "RD-M3-017", "RD-M3-018",
    "RD-M3-019", "RD-M3-020", "RD-M3-021", "RD-M3-022", "RD-M3-023", "RD-M3-024",
    "RD-M3-025"
)

$allTasks = $m2Tasks + $m3Tasks
$completed = 0

foreach ($taskName in $allTasks) {
    $query = @{
        query = "query { project(id: `"$projectId`") { issues(first: 10, filter: { title: { contains: `"$taskName`" } }) { nodes { id identifier title state { name } } } } }"
    } | ConvertTo-Json -Compress

    $response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
    $issue = $response.data.project.issues.nodes | Where-Object { $_.title -match "^$taskName" } | Select-Object -First 1

    if ($issue -and $issue.state.name -ne "Done") {
        Write-Host "Completing: $($issue.identifier) - $($issue.title)"
        $body = @{
            query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
            variables = @{ issueId = $issue.id; stateId = $doneStateId }
        } | ConvertTo-Json -Depth 5 -Compress
        
        $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $body
        if ($result.data.issueUpdate.success) {
            $completed++
            Write-Host "  -> Done"
        }
        Start-Sleep -Milliseconds 100
    }
}

Write-Host "`nCompleted $completed tasks in Linear"
