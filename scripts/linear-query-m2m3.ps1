$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

foreach ($milestone in @("M2", "M3")) {
    Write-Host "`n=== $milestone Tasks ==="
    $query = @{
        query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"$milestone`" } }) { nodes { id identifier title state { name } } } } }"
    } | ConvertTo-Json -Compress

    $response = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $query
    $issues = $response.data.project.issues.nodes | Where-Object {
        ($_.title -match "^RD-$milestone-" -or $_.title -match "^QA-$milestone-" -or $_.title -match "^GATE-$milestone" -or $_.title -match "^$milestone Epic") -and
        $_.state.name -ne "Canceled"
    }

    $grouped = $issues | Group-Object { $_.state.name }
    foreach ($group in $grouped) {
        Write-Host "--- $($group.Name) ($($group.Count)) ---"
        foreach ($issue in ($group.Group | Sort-Object { $_.identifier })) {
            Write-Host "  $($issue.identifier) | $($issue.title)"
        }
    }
    Write-Host "Total: $($issues.Count) active tasks"
}
