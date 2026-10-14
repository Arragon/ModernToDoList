$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}

$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"

# Function to update issue state
function Update-IssueState($issueId, $identifier, $stateId) {
    $mutation = @"
{
  "query": "mutation { issueUpdate(id: `"$issueId`", input: { stateId: `"$stateId`" }) { success issue { identifier state { name } } } }"
}
"@
    
    try {
        $result = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $mutation -ErrorAction Stop
        if ($result.data.issueUpdate.success) {
            Write-Host "OK: $identifier -> Done"
            return $true
        } else {
            Write-Host "FAIL: $identifier"
            return $false
        }
    } catch {
        Write-Host "ERROR: $identifier - $($_.Exception.Message)"
        return $false
    }
}

# Query M0 issues to get their IDs
$query = @'
{
  "query": "query { project(id: \"72e576a7-e894-4e22-b52c-5d47d43a5912\") { issues(first: 100, filter: { title: { contains: \"M0\" } }) { nodes { id identifier title state { name } } } } }"
}
'@

$response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
$issues = $response.data.project.issues.nodes

Write-Host "Found $($issues.Count) M0-related issues"

# Filter to only RD-M0, QA-M0, and M0 Epic tasks (exclude canceled and cross-references)
$m0Tasks = $issues | Where-Object {
    $_.title -match "^RD-M0-" -or 
    $_.title -match "^QA-M0-" -or 
    $_.title -match "^M0 Epic" -or
    $_.title -match "^GATE-M0"
} | Where-Object {
    $_.state.name -ne "Canceled"
}

Write-Host "Will update $($m0Tasks.Count) M0 tasks to Done`n"

$updated = 0
foreach ($task in $m0Tasks) {
    if (Update-IssueState $task.id $task.identifier $doneStateId) {
        $updated++
    }
    Start-Sleep -Milliseconds 200  # Rate limiting
}

Write-Host "`nM0 Summary: $updated/$($m0Tasks.Count) tasks updated to Done"
