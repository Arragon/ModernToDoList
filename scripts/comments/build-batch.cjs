'use strict';

// Render a results manifest into a scripts/linear-sync.cjs batch file.
//
// Keeping comment composition here (rather than hand-writing 90 JSON blobs)
// guarantees every issue gets the same structure, and because the manifest and
// the output are both read/written as UTF-8 by Node, Chinese bodies can never
// be corrupted by a platform codepage.
//
// Usage: node scripts/comments/build-batch.cjs <manifest.json> <out-batch.json>
//
// Manifest shape:
// {
//   "milestone": "M10",
//   "commit": "abc1234",
//   "evidence": { "cargo": "257 passed; 0 failed", "frontend": "npm run build ok" },
//   "issues": {
//     "INH-1110": {
//       "state": "Done",            // optional; omit to leave the state alone
//       "rd": ["RD-M10-001"],       // optional task ids
//       "impl": ["line", "line"],   // implementation detail bullets
//       "tests": ["line"],          // test/acceptance bullets
//       "gap": "what is NOT done"   // optional honest gap statement
//     }
//   }
// }

const fs = require('fs');
const path = require('path');

const [manifestPath, outPath] = process.argv.slice(2);
if (!manifestPath || !outPath) {
  console.error('Usage: node scripts/comments/build-batch.cjs <manifest.json> <out-batch.json>');
  process.exit(2);
}

const manifest = JSON.parse(fs.readFileSync(path.resolve(manifestPath), 'utf8'));
const issues = manifest.issues || {};
const ids = Object.keys(issues);
if (!ids.length) {
  console.error('Manifest contains no issues.');
  process.exit(2);
}

function bullets(lines) {
  return (lines || []).map((l) => `- ${l}`).join('\n');
}

function render(id, entry) {
  const parts = [];
  const marker = `<!-- mtl:${(entry.rd && entry.rd[0]) || id} -->`;

  const header = [];
  if (manifest.milestone) header.push(`**里程碑 / Milestone:** ${manifest.milestone}`);
  if (entry.rd && entry.rd.length) header.push(`**任务 / Tasks:** ${entry.rd.join(', ')}`);
  if (manifest.commit) header.push(`**提交 / Commit:** \`${manifest.commit}\``);
  parts.push(`## 进展同步 / Progress Sync\n\n${header.join('  \n')}`);

  if (entry.impl && entry.impl.length) {
    parts.push(`### 实现方案\n\n${bullets(entry.impl)}`);
  }

  if (entry.tests && entry.tests.length) {
    parts.push(`### 测试方案与验收结果\n\n${bullets(entry.tests)}`);
  }

  const evidence = manifest.evidence || {};
  const evidenceLines = [];
  if (evidence.cargo) evidenceLines.push(`- \`cargo test\`：${evidence.cargo}`);
  if (evidence.frontend) evidenceLines.push(`- 前端构建：${evidence.frontend}`);
  if (evidence.extra) evidenceLines.push(...evidence.extra.map((l) => `- ${l}`));
  if (evidenceLines.length) {
    parts.push(`### 验证证据\n\n${evidenceLines.join('\n')}`);
  }

  if (entry.gap) {
    parts.push(`### 未完成项与阻塞原因\n\n${entry.gap}`);
  }

  parts.push(
    '> 本评论由 `scripts/linear-sync.cjs` 通过 Node.js GraphQL 客户端提交，' +
      '请求体经 `JSON.stringify` → `Buffer.from(…, "utf8")` 编码，' +
      '`Content-Length` 取自 `Buffer.byteLength`，全程不经过 PowerShell，因此不存在中文乱码。'
  );

  return `${marker}\n${parts.join('\n\n')}`;
}

const updates = ids
  .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
  .map((id) => {
    const entry = issues[id];
    const update = { identifier: id, marker: `<!-- mtl:${(entry.rd && entry.rd[0]) || id} -->`, comment: render(id, entry) };
    if (entry.state) update.state = entry.state;
    return update;
  });

fs.mkdirSync(path.dirname(path.resolve(outPath)), { recursive: true });
fs.writeFileSync(path.resolve(outPath), JSON.stringify({ updates }, null, 2), 'utf8');

const byState = new Map();
for (const u of updates) byState.set(u.state || '(unchanged)', (byState.get(u.state || '(unchanged)') || 0) + 1);

console.log(`Wrote ${updates.length} update(s) to ${outPath}`);
for (const [state, count] of [...byState.entries()].sort()) console.log(`  ${state}: ${count}`);

// Guard against a manifest that claims Done without any supporting evidence.
const unsupported = updates.filter((u) => u.state === 'Done' && !/### 测试方案与验收结果/.test(u.comment));
if (unsupported.length) {
  console.error(`\nREFUSING: ${unsupported.length} issue(s) marked Done with no test/acceptance section:`);
  unsupported.forEach((u) => console.error(`  ${u.identifier}`));
  process.exit(1);
}
