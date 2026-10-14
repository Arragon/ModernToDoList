$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$projectId = "72e576a7-e894-4e22-b52c-5d47d43a5912"

# Query all M2 tasks
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 50, filter: { title: { contains: `"RD-M2`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m2 = $r.data.project.issues.nodes

Write-Host "=== M2 Tasks Status ==="
$m2Done = ($m2 | Where-Object { $_.state.name -eq "Done" }).Count
$m2IP = ($m2 | Where-Object { $_.state.name -eq "In Progress" }).Count
$m2BL = ($m2 | Where-Object { $_.state.name -eq "Backlog" }).Count
$m2C = ($m2 | Where-Object { $_.state.name -eq "Canceled" }).Count
Write-Host "Done: $m2Done | In Progress: $m2IP | Backlog: $m2BL | Canceled: $m2C | Total: $($m2.Count)"

# Query all M3 tasks
$q = @{
    query = "query { project(id: `"$projectId`") { issues(first: 100, filter: { title: { contains: `"RD-M3`" } }) { nodes { id identifier title state { name } } } } }"
} | ConvertTo-Json -Compress
$r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $q
$m3 = $r.data.project.issues.nodes

Write-Host "`n=== M3 Tasks Status ==="
$m3Done = ($m3 | Where-Object { $_.state.name -eq "Done" }).Count
$m3IP = ($m3 | Where-Object { $_.state.name -eq "In Progress" }).Count
$m3BL = ($m3 | Where-Object { $_.state.name -eq "Backlog" }).Count
$m3C = ($m3 | Where-Object { $_.state.name -eq "Canceled" }).Count
Write-Host "Done: $m3Done | In Progress: $m3IP | Backlog: $m3BL | Canceled: $m3C | Total: $($m3.Count)"

# Show any remaining non-Done, non-Canceled M2/M3 tasks
$remaining = $m2 + $m3 | Where-Object { $_.state.name -ne "Done" -and $_.state.name -ne "Canceled" }
if ($remaining.Count -gt 0) {
    Write-Host "`n=== Remaining Active Tasks ==="
    $remaining | ForEach-Object {
        Write-Host "  $($_.identifier) | $($_.state.name) | $($_.title.Substring(0, [Math]::Min(60, $_.title.Length)))"
    }
} else {
    Write-Host "`nAll M2/M3 tasks are either Done or Canceled!"
}

Write-Host "`n=== VERIFICATION SUMMARY ==="
Write-Host "M2: All $($m2.Count) tasks -> Done: $([bool]($m2IP -eq 0 -and $m2BL -eq 0))"
Write-Host "M3: Done=$m3Done, Canceled=$m3C, Active=$($m3IP + $m3BL): $([bool]($m3IP -eq 0 -and $m3BL -eq 0))"
