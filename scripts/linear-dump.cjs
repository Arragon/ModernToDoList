'use strict';

// Dump the full Linear project state to JSON + a human-readable summary.
// Usage: node scripts/linear-dump.cjs [--filter M10] [--out scripts/_linear_state.json]

const fs = require('fs');
const path = require('path');
const L = require('./lib/linear-client.cjs');

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

(async () => {
  const filter = arg('filter', null);
  const outPath = path.resolve(arg('out', 'scripts/_linear_state.json'));

  const issues = await L.fetchProjectIssues();
  const states = await L.fetchWorkflowStates();

  console.error(`Fetched ${issues.length} issues; workflow states: ${[...states.keys()].join(', ')}`);

  const selected = filter ? issues.filter((i) => i.title.includes(filter)) : issues;

  fs.writeFileSync(outPath, JSON.stringify({ fetchedAt: new Date().toISOString(), issues }, null, 2), 'utf8');

  const byState = new Map();
  for (const issue of selected) {
    const key = issue.state.name;
    if (!byState.has(key)) byState.set(key, []);
    byState.get(key).push(issue);
  }

  const lines = [];
  lines.push(`# Linear project state (filter=${filter || 'none'})`);
  lines.push(`Total in project: ${issues.length} | matching filter: ${selected.length}`);
  lines.push('');
  lines.push('## By state');
  for (const [state, list] of [...byState.entries()].sort((a, b) => b[1].length - a[1].length)) {
    lines.push(`- ${state}: ${list.length}`);
  }
  lines.push('');
  lines.push('## Issues');
  for (const [state, list] of [...byState.entries()].sort()) {
    lines.push('');
    lines.push(`### ${state} (${list.length})`);
    for (const issue of list.sort((a, b) => a.identifier.localeCompare(b.identifier, undefined, { numeric: true }))) {
      lines.push(`- ${issue.identifier} [P${issue.priority}] ${issue.title}`);
    }
  }

  const summaryPath = outPath.replace(/\.json$/, '.md');
  fs.writeFileSync(summaryPath, lines.join('\n'), 'utf8');
  console.log(lines.join('\n'));
})().catch((e) => {
  console.error('Fatal:', e.message);
  process.exit(1);
});
