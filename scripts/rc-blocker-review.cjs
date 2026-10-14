'use strict';

// RC-M10 (INH-1132) blocker classification.
//
// Pulls the live Linear project state, selects every M6-M10 issue that is not
// Done/Canceled/Duplicate, and classifies it into the five RC blocker
// categories defined by the GA gate:
//   Blocker-Data / Blocker-Persistence / Blocker-Portable / Blocker-Security / Blocker-Core-UX
//
// GATE-M10 (INH-1133) requires the blocker count to be 0, so this report is the
// evidence artifact for that check. It never marks anything Done on its own.
//
// Usage: node scripts/rc-blocker-review.cjs [--state scripts/_linear_state.json] [--out docs/release/RC_M10_BLOCKER_REVIEW.md]

const fs = require('fs');
const path = require('path');
const L = require('./lib/linear-client.cjs');

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

// Ordered: the first matching rule wins, so put the most specific first.
const RULES = [
  ['Blocker-Security', /sanitiz|xss|dangerous url|url security|url-security|privacy|crash log|diagnostic|antivirus|false-positive|signing/i],
  ['Blocker-Data', /transfer|copy|move|attachment|asset|orphan|trash|recovery|journal|atomic|crash|conflict|remap|duplicate|data safety/i],
  ['Blocker-Persistence', /xml|encoding|round-trip|round trip|unknown|comment|migration|index|sqlite|fts|workspace|watcher|serializ|persist|saved view/i],
  ['Blocker-Portable', /portable|webview2|release|zip|license|path|removable|long path|unicode path|version metadata|offline/i],
  ['Blocker-Core-UX', /tree|inspector|keyboard|undo|redo|search|view|quick add|palette|ui|editor|toolbar|virtualiz|render|participant|dependency|progress link|filter/i],
];

const CATEGORIES = ['Blocker-Data', 'Blocker-Persistence', 'Blocker-Portable', 'Blocker-Security', 'Blocker-Core-UX'];

function classify(title) {
  for (const [category, pattern] of RULES) {
    if (pattern.test(title)) return category;
  }
  return 'Blocker-Core-UX';
}

function milestoneOf(title) {
  const m = title.match(/\bM(6|7|8|9|10)\b/);
  return m ? `M${m[1]}` : 'M10';
}

const PRIORITY_LABEL = { 0: 'None', 1: 'Urgent', 2: 'High', 3: 'Medium', 4: 'Low', 5: 'None' };

(async () => {
  const outPath = path.resolve(arg('out', 'docs/release/RC_M10_BLOCKER_REVIEW.md'));
  const stateArg = arg('state', null);

  let issues;
  if (stateArg && fs.existsSync(path.resolve(stateArg))) {
    issues = JSON.parse(fs.readFileSync(path.resolve(stateArg), 'utf8')).issues;
    console.error(`Using cached state: ${stateArg} (${issues.length} issues)`);
  } else {
    issues = await L.fetchProjectIssues();
    console.error(`Fetched live state (${issues.length} issues)`);
  }

  const CLOSED = new Set(['Done', 'Canceled', 'Duplicate']);
  const inScope = issues.filter((i) => {
    if (CLOSED.has(i.state.name)) return false;
    return /\bM(6|7|8|9|10)\b/.test(i.title) || /^INH-(10[4-9]\d|11[0-3]\d)$/.test(i.identifier);
  });

  const byCategory = new Map(CATEGORIES.map((c) => [c, []]));
  for (const issue of inScope) {
    byCategory.get(classify(issue.title)).push(issue);
  }

  const p1 = inScope.filter((i) => i.priority === 1 || i.priority === 2);
  const lines = [];
  lines.push('# RC-M10 Blocker Review and 2.0 Scope Freeze');
  lines.push('');
  lines.push(`Generated: ${new Date().toISOString()}`);
  lines.push(`Source: Linear project ${L.PROJECT_ID}`);
  lines.push('');
  lines.push('## Verdict');
  lines.push('');
  lines.push(`**Open M6-M10 issues: ${inScope.length}. Blocker count: ${inScope.length}.**`);
  lines.push('');
  lines.push(
    inScope.length === 0
      ? 'Blocker count is 0 — the RC freeze precondition of GATE-M10 is satisfied.'
      : 'GATE-M10 (INH-1133) requires **all Blockers = 0** and **all P0/P1 data-safety issues = 0**. ' +
        'That precondition is NOT satisfied, so the GA gate cannot be passed and must remain open.'
  );
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push('| Category | Open | P1/Urgent+High |');
  lines.push('|----------|------|----------------|');
  for (const category of CATEGORIES) {
    const list = byCategory.get(category);
    lines.push(`| ${category} | ${list.length} | ${list.filter((i) => i.priority === 1 || i.priority === 2).length} |`);
  }
  lines.push(`| **Total** | **${inScope.length}** | **${p1.length}** |`);
  lines.push('');
  lines.push('## By milestone');
  lines.push('');
  const byMilestone = new Map();
  for (const issue of inScope) {
    const key = milestoneOf(issue.title);
    byMilestone.set(key, (byMilestone.get(key) || 0) + 1);
  }
  lines.push('| Milestone | Open issues |');
  lines.push('|-----------|-------------|');
  for (const [milestone, count] of [...byMilestone.entries()].sort()) {
    lines.push(`| ${milestone} | ${count} |`);
  }
  lines.push('');

  for (const category of CATEGORIES) {
    const list = byCategory.get(category).sort((a, b) =>
      a.identifier.localeCompare(b.identifier, undefined, { numeric: true })
    );
    lines.push(`## ${category} (${list.length})`);
    lines.push('');
    if (!list.length) {
      lines.push('_None open._');
    } else {
      lines.push('| Issue | Pri | State | Title |');
      lines.push('|-------|-----|-------|-------|');
      for (const issue of list) {
        lines.push(
          `| ${issue.identifier} | ${PRIORITY_LABEL[issue.priority] || issue.priority} | ${issue.state.name} | ${issue.title.replace(/\|/g, '\\|')} |`
        );
      }
    }
    lines.push('');
  }

  lines.push('## Gates that cannot be auto-verified in this environment');
  lines.push('');
  lines.push(
    '- **QA-M10-H (INH-1131)** requires execution on clean Windows 10 and Windows 11 VMs with no network, ' +
      'no administrator rights, and no Node/Rust toolchain installed. That is a manual, out-of-band activity ' +
      'and cannot be automated from a development workstation. Until it is executed by a human and the results ' +
      'recorded, GATE-M10 must not be closed.'
  );
  lines.push(
    '- **RC-M10 (INH-1132)** additionally requires freezing the commit, the version string and the release ZIP ' +
      'SHA-256. Those can only be frozen once the blocker count above reaches 0.'
  );
  lines.push('');

  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, lines.join('\n'), 'utf8');
  console.error(`Wrote ${outPath}`);
  console.log(`Open M6-M10 issues: ${inScope.length}`);
  for (const category of CATEGORIES) {
    console.log(`  ${category}: ${byCategory.get(category).length}`);
  }
})().catch((e) => {
  console.error('Fatal:', e.message);
  process.exit(1);
});
