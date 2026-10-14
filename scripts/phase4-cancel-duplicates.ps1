$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$cancelledStateId = "a5b16006-061a-48d8-994a-c36e45fce1b7"

function Update-State($issueId, $stateId) {
    $payload = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $issueId; stateId = $stateId }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.issueUpdate.success
}

# Remaining 24 duplicates (INH-838 already cancelled)
$duplicates = @(
    @{ id = "5cee07d9-3312-4916-943a-0b29d953ebff"; name = "INH-839 RD-M3-002" }
    @{ id = "11750a30-af3c-4b65-abcf-89109437d073"; name = "INH-840 RD-M3-003" }
    @{ id = "e43735fd-e10b-4ce5-9b77-9d06ebd63767"; name = "INH-841 RD-M3-004" }
    @{ id = "44964566-8f74-4117-8b7d-2ad293ffc532"; name = "INH-842 RD-M3-005" }
    @{ id = "20e5c431-9d61-41b8-9fc3-1c04105d6ff3"; name = "INH-843 RD-M3-006" }
    @{ id = "59ae6118-e22e-49d9-be7d-b4039a4e3a78"; name = "INH-844 RD-M3-007" }
    @{ id = "f242daa5-273b-4402-85f0-076c22303783"; name = "INH-845 RD-M3-008" }
    @{ id = "0c884bd9-1daa-4bec-aa26-0c42f9285a29"; name = "INH-846 RD-M3-009" }
    @{ id = "10486aa7-3518-4fce-b6b0-e14452c83c6a"; name = "INH-847 RD-M3-010" }
    @{ id = "e4cd7df5-bc72-4267-9728-8dcc45c8829c"; name = "INH-848 RD-M3-011" }
    @{ id = "21eebe89-807c-401b-94f8-5f76a67ada26"; name = "INH-849 RD-M3-012" }
    @{ id = "0c4db184-3589-4a75-9804-ce7fda9ea766"; name = "INH-850 RD-M3-013" }
    @{ id = "846b76ae-c293-485c-ad33-6798f660a86f"; name = "INH-851 RD-M3-014" }
    @{ id = "c4ded91f-ced8-446b-9e5a-8011ee7fe0c1"; name = "INH-852 RD-M3-015" }
    @{ id = "050dec8e-1b3c-490d-8ddf-1b8e0a218195"; name = "INH-853 RD-M3-016" }
    @{ id = "ce42d5b7-ccf6-4429-982e-cab92553884c"; name = "INH-854 RD-M3-017" }
    @{ id = "b4403ef4-500a-41a7-9ebe-967bcf5b63b5"; name = "INH-855 RD-M3-018" }
    @{ id = "786fa746-5a90-4662-afb5-c7a7a143d35d"; name = "INH-856 RD-M3-019" }
    @{ id = "010b4bd9-166f-4497-bbfe-7fbbda39e33e"; name = "INH-857 RD-M3-020" }
    @{ id = "83a1097b-4307-4237-8abd-1546e2ad1cae"; name = "INH-858 RD-M3-021" }
    @{ id = "86f886b4-e5fd-4b46-b91e-7f2dd10e5653"; name = "INH-859 RD-M3-022" }
    @{ id = "be3c77dc-f078-4f8d-96c8-7b03d03fb80d"; name = "INH-860 RD-M3-023" }
    @{ id = "2e8f5c9c-3811-46de-87c1-13cfbb753066"; name = "INH-861 RD-M3-024" }
    @{ id = "1e40dddd-24d7-4008-949c-e55529f9ccff"; name = "INH-862 RD-M3-025" }
)

Write-Host "=== Cancelling 24 remaining duplicate M3 Backlog tasks ==="
$success = 0
$fail = 0

foreach ($dup in $duplicates) {
    Write-Host "  Cancelling: $($dup.name)"
    $ok = Update-State $dup.id $cancelledStateId
    if ($ok) {
        $success++
        Write-Host "    -> OK"
    } else {
        $fail++
        Write-Host "    -> FAILED"
    }
    Start-Sleep -Milliseconds 200
}

Write-Host "`nResult: $success/24 cancelled (+ 1 INH-838 already fixed = 25 total)"
