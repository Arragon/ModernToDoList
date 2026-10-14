'use strict';

// Idempotent, UTF-8-safe Linear progress sync.
//
// Usage:
//   node scripts/linear-sync.cjs <batch-file.json> [--dry-run]
//
// Batch file shape:
// {
//   "updates": [
//     { "identifier": "INH-1110",
//       "state": "Done",                  // optional workflow state name
//       "marker": "<!-- mtl:RD-M10-001 -->", // optional idempotency marker
//       "comment": "markdown body (UTF-8, Chinese OK)"  // optional
//     }
//   ]
// }
//
// Anti-mojibake contract: every request body is produced by JSON.stringify and
// encoded to a Buffer with explicit 'utf8' inside lib/linear-client.cjs, and
// batch files are read with fs.readFileSync(..., 'utf8'). No PowerShell and no
// platform codepage is ever involved, so Chinese text cannot be corrupted.

const fs = require('fs');
const path = require('path');
const L = require('./lib/linear-client.cjs');

const DRY = process.argv.includes('--dry-run');

// Comments previously posted through PowerShell arrived as "## ?? ??????".
const GARBLED = /^\s*##\s*\?\?/m;

function loadBatch(file) {
  const raw = fs.readFileSync(path.resolve(file), 'utf8');
  const parsed = JSON.parse(raw);
  if (!parsed || !Array.isArray(parsed.updates)) {
    throw new Error(`Batch file must contain an "updates" array: ${file}`);
  }
  return parsed.updates;
}

async function syncOne(issue, update) {
  const label = `${issue.identifier}`;
  const actions = [];

  // 1. Remove mojibake leftovers from the earlier PowerShell-based syncs.
  const existing = await L.listComments(issue.id);
  const garbled = existing.filter((c) => GARBLED.test(c.body || ''));
  for (const bad of garbled) {
    if (!DRY) await L.deleteComment(bad.id);
    actions.push(`deleted ${bad.id.slice(0, 8)} (mojibake)`);
    await L.sleep(120);
  }

  // 2. Upsert the progress comment, keyed by marker to stay idempotent.
  if (update.comment) {
    const marker = update.marker || null;
    const body = marker && !update.comment.includes(marker) ? `${marker}\n${update.comment}` : update.comment;

    // Sanity-check the round trip before sending anything to the API.
    const reencoded = Buffer.from(JSON.stringify({ body }), 'utf8').toString('utf8');
    if (!JSON.parse(reencoded).body.includes(update.comment.slice(0, 24))) {
      throw new Error(`${label}: UTF-8 round-trip check failed — refusing to post a corrupted body`);
    }

    const prior = marker
      ? existing.find((c) => (c.body || '').includes(marker) && !GARBLED.test(c.body || ''))
      : null;

    if (prior) {
      if (!DRY) await L.updateComment(prior.id, body);
      actions.push(`updated comment ${prior.id.slice(0, 8)}`);
    } else {
      if (!DRY) {
        const ok = await L.addComment(issue.id, body);
        if (!ok) throw new Error(`${label}: commentCreate returned false`);
      }
      actions.push('created comment');
    }
    await L.sleep(150);
  }

  // 3. Move the workflow state last, so a failed comment never hides a state change.
  if (update.state && issue.state.name !== update.state) {
    if (!DRY) await L.setIssueState(issue.id, update.state);
    actions.push(`state ${issue.state.name} -> ${update.state}`);
  } else if (update.state) {
    actions.push(`state already ${update.state}`);
  }

  return actions;
}

(async () => {
  const batchFile = process.argv[2];
  if (!batchFile) {
    console.error('Usage: node scripts/linear-sync.cjs <batch-file.json> [--dry-run]');
    process.exit(2);
  }

  const updates = loadBatch(batchFile);
  console.error(`${DRY ? '[DRY RUN] ' : ''}Loaded ${updates.length} update(s) from ${batchFile}`);

  const issues = await L.fetchProjectIssues();
  const byIdentifier = new Map(issues.map((i) => [i.identifier, i]));

  let ok = 0;
  let failed = 0;
  for (const update of updates) {
    const issue = byIdentifier.get(update.identifier);
    if (!issue) {
      console.log(`MISS  ${update.identifier}: not found in project`);
      failed++;
      continue;
    }
    try {
      const actions = await syncOne(issue, update);
      console.log(`OK    ${update.identifier} [${issue.title.slice(0, 52)}] :: ${actions.join('; ') || 'no-op'}`);
      ok++;
    } catch (err) {
      console.log(`FAIL  ${update.identifier}: ${err.message}`);
      failed++;
    }
  }

  console.error(`\nDone: ${ok} ok, ${failed} failed, ${updates.length} total.`);
  if (failed) process.exit(1);
})().catch((e) => {
  console.error('Fatal:', e.message);
  process.exit(1);
});
