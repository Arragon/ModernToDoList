$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

function Update-IssueState($issueId, $identifier) {
    $body = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{
            issueId = $issueId
            stateId = $script:doneStateId
        }
    } | ConvertTo-Json -Depth 5 -Compress
    
    try {
        $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $script:headers -Body $body -ErrorAction Stop
        if ($result.data.issueUpdate.success) {
            Write-Host "OK: $identifier -> Done"
            return $true
        }
    } catch {
        Write-Host "FAIL: $identifier"
    }
    return $false
}

# Update M0 tasks
Write-Host "=== Updating M0 Tasks ==="
$m0Query = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"M0`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $m0Query
$m0Issues = $response.data.project.issues.nodes | Where-Object {
    ($_.title -match "^RD-M0-" -or $_.title -match "^QA-M0-" -or $_.title -match "^M0 Epic" -or $_.title -match "^GATE-M0") -and
    $_.state.name -ne "Canceled" -and $_.state.name -ne "Done"
}

Write-Host "Found $($m0Issues.Count) M0 tasks to update"
$m0Updated = 0
foreach ($issue in $m0Issues) {
    if (Update-IssueState $issue.id $issue.identifier) { $m0Updated++ }
    Start-Sleep -Milliseconds 150
}
Write-Host "M0: $m0Updated/$($m0Issues.Count) updated`n"

# Update M1 tasks
Write-Host "=== Updating M1 Tasks ==="
$m1Query = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"M1`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $m1Query
$m1Issues = $response.data.project.issues.nodes | Where-Object {
    ($_.title -match "^RD-M1-" -or $_.title -match "^QA-M1-" -or $_.title -match "^M1 Epic" -or $_.title -match "^GATE-M1") -and
    $_.state.name -ne "Canceled" -and $_.state.name -ne "Done"
}

Write-Host "Found $($m1Issues.Count) M1 tasks to update"
$m1Updated = 0
foreach ($issue in $m1Issues) {
    if (Update-IssueState $issue.id $issue.identifier) { $m1Updated++ }
    Start-Sleep -Milliseconds 150
}
Write-Host "M1: $m1Updated/$($m1Issues.Count) updated`n"

Write-Host "=== Summary ==="
Write-Host "M0: $m0Updated tasks -> Done"
Write-Host "M1: $m1Updated tasks -> Done"
Write-Host "Total: $($m0Updated + $m1Updated) tasks synced to Linear"
