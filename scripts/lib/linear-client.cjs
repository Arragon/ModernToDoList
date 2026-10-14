'use strict';

// UTF-8-safe Linear GraphQL client.
//
// Encoding contract (anti-mojibake):
//   - bodies are built as JS objects and serialized with JSON.stringify
//   - the payload is converted to a Buffer with an explicit 'utf8' encoding
//   - Content-Length is Buffer.byteLength(payload), never payload.length
//   - responses are decoded with res.setEncoding('utf8')
// This avoids the Windows PowerShell default-codepage (GBK/CP936) corruption
// that previously produced "## ?? ?????" comments in Linear.

const https = require('https');
const fs = require('fs');
const path = require('path');

const PROJECT_ID = '72e576a7-e894-4e22-b52c-5d47d43a5912';

function loadEnvFile() {
  const candidates = [
    path.join(__dirname, '..', '..', '.env'),
    path.join(__dirname, '..', '.env'),
  ];
  for (const file of candidates) {
    if (!fs.existsSync(file)) continue;
    for (const rawLine of fs.readFileSync(file, 'utf8').split(/\r?\n/)) {
      const line = rawLine.trim();
      if (!line || line.startsWith('#')) continue;
      const eq = line.indexOf('=');
      if (eq < 0) continue;
      const key = line.slice(0, eq).trim();
      let value = line.slice(eq + 1).trim();
      if (/^(".*"|'.*')$/s.test(value)) value = value.slice(1, -1);
      if (process.env[key] === undefined) process.env[key] = value;
    }
    return;
  }
}

loadEnvFile();

const API_KEY = process.env.LINEAR_API_KEY;
if (!API_KEY) {
  console.error('ERROR: LINEAR_API_KEY is not set.');
  console.error('Create a .env file at the repo root containing:');
  console.error('  LINEAR_API_KEY=lin_api_...');
  process.exit(2);
}

function graphql(query, variables) {
  const payload = Buffer.from(JSON.stringify({ query, variables: variables || {} }), 'utf8');
  return new Promise((resolve, reject) => {
    const req = https.request(
      {
        hostname: 'api.linear.app',
        path: '/graphql',
        method: 'POST',
        headers: {
          Authorization: API_KEY,
          'Content-Type': 'application/json; charset=utf-8',
          'Content-Length': Buffer.byteLength(payload),
        },
      },
      (res) => {
        let data = '';
        res.setEncoding('utf8');
        res.on('data', (chunk) => (data += chunk));
        res.on('end', () => {
          try {
            resolve(JSON.parse(data));
          } catch (err) {
            reject(new Error(`Non-JSON response (${res.statusCode}): ${data.slice(0, 300)}`));
          }
        });
      }
    );
    req.on('error', reject);
    req.write(payload);
    req.end();
  });
}

async function throwIfErrors(result, label) {
  if (result && result.errors) {
    throw new Error(`${label || 'GraphQL'} errors: ${JSON.stringify(result.errors)}`);
  }
  return result;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchProjectIssues() {
  const all = [];
  let after = null;
  for (;;) {
    const result = await throwIfErrors(
      await graphql(
        `query ($id: String!, $after: String) {
           project(id: $id) {
             issues(first: 100, after: $after, orderBy: createdAt) {
               nodes { id identifier number title priority estimate state { name type } }
               pageInfo { hasNextPage endCursor }
             }
           }
         }`,
        { id: PROJECT_ID, after }
      ),
      'fetchProjectIssues'
    );
    const page = result.data.project.issues;
    all.push(...page.nodes);
    if (!page.pageInfo.hasNextPage) break;
    after = page.pageInfo.endCursor;
  }
  return all;
}

async function listComments(issueId) {
  const result = await throwIfErrors(
    await graphql(
      `query ($id: String!) { issue(id: $id) { comments(first: 100) { nodes { id body createdAt } } } }`,
      { id: issueId }
    ),
    'listComments'
  );
  return result.data.issue.comments.nodes;
}

async function addComment(issueId, body) {
  const result = await throwIfErrors(
    await graphql(
      `mutation ($issueId: String!, $body: String!) {
         commentCreate(input: { issueId: $issueId, body: $body }) { success comment { id } }
       }`,
      { issueId, body }
    ),
    'addComment'
  );
  return Boolean(result.data.commentCreate && result.data.commentCreate.success);
}

async function updateComment(commentId, body) {
  const result = await throwIfErrors(
    await graphql(
      `mutation ($id: String!, $body: String!) {
         commentUpdate(id: $id, input: { body: $body }) { success }
       }`,
      { id: commentId, body }
    ),
    'updateComment'
  );
  return Boolean(result.data.commentUpdate && result.data.commentUpdate.success);
}

async function deleteComment(commentId) {
  const result = await throwIfErrors(
    await graphql(`mutation ($id: String!) { commentDelete(id: $id) { success } }`, { id: commentId }),
    'deleteComment'
  );
  return Boolean(result.data.commentDelete && result.data.commentDelete.success);
}

const STATE_CACHE = new Map();

async function fetchWorkflowStates() {
  if (STATE_CACHE.size) return STATE_CACHE;
  const result = await throwIfErrors(
    await graphql(
      `query ($id: String!) {
         project(id: $id) { teams { nodes { id name states { nodes { id name type position } } } } }
       }`,
      { id: PROJECT_ID }
    ),
    'fetchWorkflowStates'
  );
  for (const team of result.data.project.teams.nodes) {
    for (const state of team.states.nodes) {
      STATE_CACHE.set(state.name, state);
    }
  }
  return STATE_CACHE;
}

async function issueTeamId(issueId) {
  const result = await throwIfErrors(
    await graphql(`query ($id: String!) { issue(id: $id) { id team { id name } } }`, { id: issueId }),
    'issueTeamId'
  );
  return result.data.issue.team;
}

async function setIssueState(issueId, stateName) {
  const states = await fetchWorkflowStates();
  const state = states.get(stateName);
  if (!state) throw new Error(`Unknown workflow state "${stateName}". Known: ${[...states.keys()].join(', ')}`);
  const result = await throwIfErrors(
    await graphql(
      `mutation ($id: String!, $stateId: String!) {
         issueUpdate(id: $id, input: { stateId: $stateId }) { success }
       }`,
      { id: issueId, stateId: state.id }
    ),
    'setIssueState'
  );
  return Boolean(result.data.issueUpdate && result.data.issueUpdate.success);
}

async function findIssueByIdentifier(identifier) {
  const result = await throwIfErrors(
    await graphql(
      `query ($id: String!) { issue(id: $id) { id identifier title state { name } priority } }`,
      { id: identifier }
    ).catch(() => ({ errors: [] })),
    'findIssueByIdentifier'
  );
  if (result.data && result.data.issue) return result.data.issue;
  const issues = await fetchProjectIssues();
  return issues.find((i) => i.identifier === identifier) || null;
}

module.exports = {
  PROJECT_ID,
  graphql,
  throwIfErrors,
  sleep,
  fetchProjectIssues,
  listComments,
  addComment,
  updateComment,
  deleteComment,
  fetchWorkflowStates,
  issueTeamId,
  setIssueState,
  findIssueByIdentifier,
};
