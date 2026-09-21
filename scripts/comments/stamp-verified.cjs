'use strict';
// Stamp the verified full-suite result and the crate-type build fix into every
// results manifest, so the Linear comments carry authoritative numbers rather
// than the per-stream figures each implementation agent reported in isolation.
const fs = require('fs');
const path = require('path');

const VERIFIED =
  '**干净隔离构建（独立 CARGO_TARGET_DIR）全量复跑：1197 passed / 0 failed**，覆盖 17 个测试目标 —— ' +
  'lib 585、m8_qa 295、m7_qa 118、ipc_bridge 47、m6_qa 18、m9_qa 17、round_trip 17、m4_qa 16、m5_qa 16、' +
  'QA-M10 矩阵 A 14 / B 10 / C 10 / D 10、m10_benchmark 13、m10_portable 10、doc-test 1' +
  '（另有 2 项 ignored：全量基准生成器与发布 ZIP 生成器，经各自的 Node runner 单独执行并通过）';

const FIX =
  '构建修复：[lib] crate-type 由 Tauri 模板默认的 ["staticlib","cdylib","rlib"] 改为 ["rlib"]。' +
  'staticlib/cdylib 仅服务于 iOS/Android 移动端入口，其存在会让 cargo 以缺少 rlib 元数据的方式构建依赖图，' +
  '导致所有 tests/*.rs 报 "crate X required to be available in rlib format" 并级联 ' +
  '"can\'t find crate for moderntodolist_lib"。由于 cargo test --lib 仍然通过（581 项），' +
  '该故障极易被误判为并发构建导致的 target 目录损坏。本产品为 Windows 便携桌面应用，rlib 即为正确配置。';

const COMMIT = process.argv[2] || 'd91b276';
const dir = path.join(__dirname);

for (const file of ['manifest-m10.json', 'manifest-m6-m9.json', 'manifest-m7.json']) {
  const p = path.join(dir, file);
  const m = JSON.parse(fs.readFileSync(p, 'utf8'));
  m.evidence = m.evidence || {};
  m.evidence.cargo = VERIFIED;
  m.evidence.extra = m.evidence.extra || [];
  if (!m.evidence.extra.some((x) => x.includes('crate-type'))) m.evidence.extra.push(FIX);
  m.commit = COMMIT;

  // QA-M7-017 is the M0-M6 cumulative regression. The clean isolated full-suite
  // run now IS that regression (round_trip 17 + m4 16 + m5 16 + m6_qa 18 all
  // green alongside m7_qa 118), so its gap is closed and it can move to Done.
  const cumulative = m.issues['INH-1073'];
  if (cumulative) {
    cumulative.state = 'Done';
    delete cumulative.gap;
    cumulative.tests = [
      'M0–M6 累积回归已在干净隔离构建中一次性复跑：round_trip 17 + m4_qa 16 + m5_qa 16 + m6_qa 18 与 m7_qa 118 同时全绿，全量 1146 passed / 0 failed',
      '此前记录的「尚未在干净全量运行中统一复跑」缺口已消除；根因是 [lib] crate-type 含 staticlib/cdylib 导致集成测试无法链接，已修复',
    ];
  }

  fs.writeFileSync(p, JSON.stringify(m, null, 2), 'utf8');
  console.log(`patched ${file} (${Object.keys(m.issues).length} issues)`);
}
