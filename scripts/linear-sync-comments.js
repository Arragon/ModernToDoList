const https = require('https');
const fs = require('fs');
const path = require('path');

const API_KEY = 'process.env.LINEAR_API_KEY';
const PROJECT_ID = '72e576a7-e894-4e22-b52c-5d47d43a5912';
const COMMENTS_FILE = path.join(__dirname, 'linear-comments.json');

// ── HTTP helper ──
function linearCall(body) {
  return new Promise((resolve, reject) => {
    const json = JSON.stringify(body);
    const req = https.request({
      hostname: 'api.linear.app',
      path: '/graphql',
      method: 'POST',
      headers: {
        'Authorization': API_KEY,
        'Content-Type': 'application/json; charset=utf-8',
        'Content-Length': Buffer.byteLength(json),
      },
    }, (res) => {
      let data = '';
      res.setEncoding('utf8');
      res.on('data', (chunk) => data += chunk);
      res.on('end', () => {
        try { resolve(JSON.parse(data)); }
        catch (e) { reject(new Error(`Parse error: ${data.substring(0, 200)}`)); }
      });
    });
    req.on('error', reject);
    req.write(json);
    req.end();
  });
}

const sleep = (ms) => new Promise(r => setTimeout(r, ms));

// ── Paginated fetch of all project issues ──
async function fetchAllIssues() {
  const all = [];
  let after = null;
  let page = 0;
  while (true) {
    page++;
    const cursorPart = after ? `, after: "${after}"` : '';
    const r = await linearCall({
      query: `query { project(id: "${PROJECT_ID}") { issues(first: 100${cursorPart}) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }`
    });
    if (r.errors) { console.error('GraphQL errors:', JSON.stringify(r.errors)); break; }
    const proj = r.data.project;
    all.push(...proj.issues.nodes);
    console.error(`  Page ${page}: ${proj.issues.nodes.length} issues`);
    if (!proj.issues.pageInfo.hasNextPage) break;
    after = proj.issues.pageInfo.endCursor;
  }
  return all;
}

// ── List comments for an issue ──
async function listComments(issueId) {
  const r = await linearCall({
    query: `query($id: String!) { issue(id: $id) { comments { nodes { id body createdAt } } } }`,
    variables: { id: issueId },
  });
  if (r.errors) { console.error('  listComments error:', JSON.stringify(r.errors)); return []; }
  return r.data.issue.comments.nodes;
}

// ── Delete a comment ──
async function deleteComment(commentId) {
  const r = await linearCall({
    query: `mutation($id: String!) { commentDelete(id: $id) { success } }`,
    variables: { id: commentId },
  });
  return r.data && r.data.commentDelete && r.data.commentDelete.success;
}

// ── Add a comment ──
async function addComment(issueId, body) {
  const r = await linearCall({
    query: `mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }`,
    variables: { issueId, body },
  });
  return r.data && r.data.commentCreate && r.data.commentCreate.success;
}

// ── Match task title to comment key ──
function matchCommentKey(title) {
  if (/^RD-M4-0(19|2[0-9]|3[0-9])/.test(title)) return 'workspace_domain';
  if (/^RD-M4-0(3[1-9]|4[0-6])/.test(title)) return 'file_watcher';
  if (/^QA-M4-/.test(title)) return 'qa_m4';
  if (/^GATE-M4/.test(title)) return 'gate_m4';
  if (/^RD-M5-00[1-9]/.test(title) || /^RD-M5-01[0-4]/.test(title)) return 'ui_infra';
  if (/^RD-M5-01[5-9]/.test(title) || /^RD-M5-02[0-9]/.test(title) || /^RD-M5-030/.test(title)) return 'task_tree';
  if (/^RD-M5-03[1-9]/.test(title) || /^RD-M5-040/.test(title)) return 'inspector';
  if (/^RD-M5-04[1-9]/.test(title) || /^RD-M5-05[0-2]/.test(title)) return 'keyboard';
  if (/^RD-M5-05[3-9]/.test(title) || /^RD-M5-06[0-4]/.test(title)) return 'filter';
  if (/^QA-M5-/.test(title)) return 'qa_m5';
  if (/^GATE-M5/.test(title)) return 'gate_m5';
  if (/^M[45] Epic/.test(title)) return 'ui_infra';
  return null;
}

// ── Detect garbled comment (from PowerShell GBK encoding bug) ──
function isGarbled(body) {
  // Garbled comments start with "## ?? ??????" (corrupted emoji + Chinese)
  return /^## \?\? \?\?\?\?\?\?/m.test(body);
}

// ── Main ──
async function main() {
  console.error('Loading comments from JSON...');
  const comments = JSON.parse(fs.readFileSync(COMMENTS_FILE, 'utf8'));
  const keys = Object.keys(comments);
  console.error(`  ${keys.length} comment templates loaded`);

  console.error('Fetching all project issues...');
  const allIssues = await fetchAllIssues();
  console.error(`  Total: ${allIssues.length} issues`);

  // Filter M4-M5 tasks (already Done, not Canceled)
  const tasks = allIssues.filter(i => {
    const isM4M5 = /^(RD-M[45]-|QA-M[45]-|GATE-M[45]|M[45] Epic)/.test(i.title);
    const notCanceled = i.state.name !== 'Canceled';
    return isM4M5 && notCanceled;
  });
  console.error(`  M4-M5 tasks found: ${tasks.length}`);

  let ok = 0, fail = 0, skip = 0;

  for (const task of tasks.sort((a, b) => a.identifier.localeCompare(b.identifier))) {
    const key = matchCommentKey(task.title);
    if (!key) {
      console.log(`SKIP (no match): ${task.identifier} - ${task.title}`);
      skip++;
      continue;
    }
    const commentBody = comments[key];
    if (!commentBody) {
      console.log(`SKIP (no template): ${task.identifier} key=${key}`);
      skip++;
      continue;
    }

    process.stdout.write(`${task.identifier}... `);

    // Step 1: Delete garbled comments
    try {
      const existing = await listComments(task.id);
      const garbled = existing.filter(c => isGarbled(c.body));
      for (const g of garbled) {
        await deleteComment(g.id);
        await sleep(100);
      }
      if (garbled.length > 0) {
        process.stdout.write(`del ${garbled.length} garbled... `);
      }

      // Check if a correct comment already exists (avoid duplicates)
      const hasCorrect = existing.some(c => c.body.includes('### 实现方案') && c.body.includes('### 测试方案与验收结果') && !isGarbled(c.body));
      if (hasCorrect) {
        console.log('SKIP (already has correct comment)');
        skip++;
        continue;
      }

      // Step 2: Add correct comment
      const success = await addComment(task.id, commentBody);
      if (success) {
        console.log('OK');
        ok++;
      } else {
        console.log('FAIL (commentCreate returned false)');
        fail++;
      }
    } catch (e) {
      console.log(`FAIL (${e.message})`);
      fail++;
    }

    await sleep(200);
  }

  console.error(`\n=== Done: ${ok} OK, ${fail} FAIL, ${skip} SKIP out of ${tasks.length} tasks ===`);
}

main().catch(e => { console.error('Fatal:', e); process.exit(1); });
