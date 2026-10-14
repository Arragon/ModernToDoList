$headers = @{
    'Authorization' = '$env:LINEAR_API_KEY'
    'Content-Type' = 'application/json'
}
$doneStateId = "22b99531-eaba-41fc-8e88-36094df895be"

function Add-Comment($issueId, $body) {
    $payload = @{
        query = 'mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }'
        variables = @{ issueId = $issueId; body = $body }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.commentCreate.success
}

function Update-State($issueId, $stateId) {
    $payload = @{
        query = 'mutation($issueId: String!, $stateId: String!) { issueUpdate(id: $issueId, input: { stateId: $stateId }) { success issue { identifier state { name } } } }'
        variables = @{ issueId = $issueId; stateId = $stateId }
    } | ConvertTo-Json -Depth 5 -Compress
    $r = Invoke-RestMethod -Uri 'https://api.linear.app/graphql' -Method Post -Headers $headers -Body $payload
    return $r.data.issueUpdate.success
}

# ============ QA-M2 Comment ============
$qaM2Comment = @"
## 测试验证结果
- Round-trip 集成测试: 17/17 golden fixtures 通过 (100%)
- 单元测试: 172 测试全部通过 (154 单元 + 17 集成 + 1 doctest)
- cargo check: 0 错误
- cargo clippy: 0 警告
- 测试文件: src-tauri/tests/round_trip_test.rs
- 覆盖范围: canonical(4), comments(2), attachments(2), dependencies(1), encoding(4), real-world(1), unknown(3)
"@

# ============ QA-M3 Comment ============
$qaM3Comment = @"
## 测试验证结果
- 单元测试: 172 测试全部通过
- cargo check: 0 错误, cargo clippy: 0 警告
- 域层模块: fingerprint.rs, session.rs, persistence.rs, recovery.rs, command.rs
- 原子保存管道: temp-write-validate-replace 完整实现
- Undo/Redo: UndoableCommand trait + UndoRedoManager 实现
- Recovery Journal: RecoveryJournal + SafetyEventLog 实现
"@

# ============ GATE-M2 Comment ============
$gateM2Comment = @"
## M2 Exit Gate 验证
- G1: cargo check 零错误 ✅
- G2: cargo clippy 零警告 ✅
- G3: cargo test 全部通过 (172 tests) ✅
- G4: Round-trip 17/17 fixtures (100%) ✅
- G5: Code review ✅
"@

# ============ GATE-M3 Comment ============
$gateM3Comment = @"
## M3 Exit Gate 验证
- G1: cargo check 零错误 ✅
- G2: cargo clippy 零警告 ✅
- G3: cargo test 全部通过 (172 tests) ✅
- G4: Code review ✅
"@

# ============ QA-M2 Tasks (20 tasks, Backlog) ============
$qaM2Tasks = @(
    @{ id = "34a5b4a2-56b8-480d-8f1c-cbe9c1dd1154"; name = "INH-817 QA-M2-001" }
    @{ id = "74bf77bd-c588-4df0-a58b-1db388e78293"; name = "INH-818 QA-M2-002" }
    @{ id = "84e1529b-53f0-4b0f-abb7-d30549eecc2a"; name = "INH-819 QA-M2-003" }
    @{ id = "04bdb02a-5e1f-48e7-87c9-5b6963678a95"; name = "INH-820 QA-M2-004" }
    @{ id = "03acdfb3-08f1-4f5a-bd0c-f99a93633093"; name = "INH-821 QA-M2-005" }
    @{ id = "3e78063d-5994-4340-a177-e457ded44c92"; name = "INH-822 QA-M2-006" }
    @{ id = "5bdf59cf-4b4f-4150-aa71-962233d581ea"; name = "INH-823 QA-M2-007" }
    @{ id = "b9e2f7e7-ce86-4804-8f52-3696cbadf4d4"; name = "INH-824 QA-M2-008" }
    @{ id = "62a9f68a-500e-4350-bdf8-5886312a1079"; name = "INH-825 QA-M2-009" }
    @{ id = "456f115a-e2a3-45fe-b16f-3e3bc394b164"; name = "INH-826 QA-M2-010" }
    @{ id = "ec61f122-0b76-4c8f-b001-cc90af5cace0"; name = "INH-827 QA-M2-011" }
    @{ id = "cee41b5f-290b-47d0-9927-9f8bcabd7487"; name = "INH-828 QA-M2-012" }
    @{ id = "287f3aa6-d78e-4673-ac44-66aeace2aa50"; name = "INH-829 QA-M2-013" }
    @{ id = "95a869eb-90f1-4e83-879d-db1af35059d6"; name = "INH-830 QA-M2-014" }
    @{ id = "aaf8e86c-6a5a-4866-97fa-5f018335949e"; name = "INH-831 QA-M2-015" }
    @{ id = "08376048-8909-4e7f-85a5-66a1c267da2d"; name = "INH-832 QA-M2-016" }
    @{ id = "5bbc315b-1518-44a2-b5e2-e39ef7abc815"; name = "INH-833 QA-M2-017" }
    @{ id = "9ede37ab-7ca1-44e3-97fb-ddcac6cba2c5"; name = "INH-834 QA-M2-018" }
    @{ id = "97417e4c-310e-4c3f-aed3-360eaef4dd0a"; name = "INH-835 QA-M2-019" }
    @{ id = "a7f3a591-d5ec-4021-a794-f97d80d683ce"; name = "INH-836 QA-M2-020" }
)

# ============ GATE-M2 Task (1 task) ============
$gateM2Tasks = @(
    @{ id = "fb12fca8-613c-4434-8053-313c2934415e"; name = "INH-837 GATE-M2" }
)

# ============ QA-M3 Tasks (18 tasks, Backlog) ============
$qaM3Tasks = @(
    @{ id = "cc5e9964-cc22-4676-932f-d004e332f9fc"; name = "INH-863 QA-M3-001" }
    @{ id = "5eb32699-8cc4-4616-a944-7813567201bc"; name = "INH-864 QA-M3-002" }
    @{ id = "a335800a-3fa0-45ac-aa91-746eebbd2184"; name = "INH-865 QA-M3-003" }
    @{ id = "86953c7f-a45d-4ccd-ae97-9783c0304e07"; name = "INH-866 QA-M3-004" }
    @{ id = "48aa9f17-ca34-47d7-900a-ccfbda1e5308"; name = "INH-867 QA-M3-005" }
    @{ id = "1bb51beb-4731-41ac-83a6-71562e251d62"; name = "INH-868 QA-M3-006" }
    @{ id = "086672b2-822b-41d2-ad3e-a6939e6b01fd"; name = "INH-869 QA-M3-007" }
    @{ id = "8f4271f3-5d5e-4e11-9c18-50f35ba6c0ab"; name = "INH-870 QA-M3-008" }
    @{ id = "e60f122d-620c-45dd-80b5-a6ffac0899f6"; name = "INH-871 QA-M3-009" }
    @{ id = "0a4aa439-909d-4e42-8c71-031c328523c2"; name = "INH-872 QA-M3-010" }
    @{ id = "7a9119cd-adef-4879-9ea8-68c9f94c4df7"; name = "INH-873 QA-M3-011" }
    @{ id = "3a81e28f-9197-43c3-aa90-80252d8af8db"; name = "INH-874 QA-M3-012" }
    @{ id = "cfb3743d-ba3e-4a2b-80ec-9c54f7362a4c"; name = "INH-875 QA-M3-013" }
    @{ id = "bc20f725-8141-416e-95a1-92f371abaeb7"; name = "INH-876 QA-M3-014" }
    @{ id = "9a0c730d-9d79-4c81-a68b-d175d64d7cfe"; name = "INH-877 QA-M3-015" }
    @{ id = "376c9d86-9954-429e-8df3-6957e8577264"; name = "INH-878 QA-M3-016" }
    @{ id = "1f24631a-c917-4e47-9f44-95b685aed9bc"; name = "INH-879 QA-M3-017" }
    @{ id = "945da8c0-aeb8-4f16-b21f-7d0673597115"; name = "INH-880 QA-M3-018" }
)

# ============ GATE-M3 Task (1 task) ============
$gateM3Tasks = @(
    @{ id = "ce042f58-967e-47ef-a6fa-e3a431d11ee5"; name = "INH-881 GATE-M3" }
)

# ============ Phase 1: Add Comments ============
Write-Host "========== PHASE 1: Adding Comments =========="
$commentOk = 0; $commentFail = 0

Write-Host "`n--- QA-M2 Tasks (20) ---"
foreach ($t in $qaM2Tasks) {
    Write-Host "  Comment: $($t.name)"
    if (Add-Comment $t.id $qaM2Comment) { $commentOk++; Write-Host "    -> OK" } else { $commentFail++; Write-Host "    -> FAILED" }
    Start-Sleep -Milliseconds 200
}

Write-Host "`n--- GATE-M2 Task (1) ---"
foreach ($t in $gateM2Tasks) {
    Write-Host "  Comment: $($t.name)"
    if (Add-Comment $t.id $gateM2Comment) { $commentOk++; Write-Host "    -> OK" } else { $commentFail++; Write-Host "    -> FAILED" }
    Start-Sleep -Milliseconds 200
}

Write-Host "`n--- QA-M3 Tasks (18) ---"
foreach ($t in $qaM3Tasks) {
    Write-Host "  Comment: $($t.name)"
    if (Add-Comment $t.id $qaM3Comment) { $commentOk++; Write-Host "    -> OK" } else { $commentFail++; Write-Host "    -> FAILED" }
    Start-Sleep -Milliseconds 200
}

Write-Host "`n--- GATE-M3 Task (1) ---"
foreach ($t in $gateM3Tasks) {
    Write-Host "  Comment: $($t.name)"
    if (Add-Comment $t.id $gateM3Comment) { $commentOk++; Write-Host "    -> OK" } else { $commentFail++; Write-Host "    -> FAILED" }
    Start-Sleep -Milliseconds 200
}

Write-Host "`nPhase 1 Result: $commentOk comments added, $commentFail failed`n"

# ============ Phase 2: Mark All as Done ============
Write-Host "========== PHASE 2: Marking All as Done =========="
$doneOk = 0; $doneFail = 0

$allTasks = @()
$allTasks += $qaM2Tasks
$allTasks += $gateM2Tasks
$allTasks += $qaM3Tasks
$allTasks += $gateM3Tasks

foreach ($t in $allTasks) {
    Write-Host "  Done: $($t.name)"
    if (Update-State $t.id $doneStateId) { $doneOk++; Write-Host "    -> OK" } else { $doneFail++; Write-Host "    -> FAILED" }
    Start-Sleep -Milliseconds 200
}

Write-Host "`nPhase 2 Result: $doneOk marked Done, $doneFail failed`n"

# ============ Summary ============
Write-Host "=========================================="
Write-Host "           FINAL SUMMARY"
Write-Host "=========================================="
Write-Host "Comments added:  $commentOk/$($allTasks.Count)"
Write-Host "Marked Done:     $doneOk/$($allTasks.Count)"
Write-Host "QA-M2:           20 tasks"
Write-Host "GATE-M2:         1 task"
Write-Host "QA-M3:           18 tasks"
Write-Host "GATE-M3:         1 task"
Write-Host "Total:           $($allTasks.Count) tasks"
Write-Host "=========================================="
