#!/usr/bin/env node
/**
 * Fix: Delete garbled comments and re-send correct UTF-8 comments via Linear API.
 * Uses Node.js native https + UTF-8 — no encoding issues.
 */

const https = require("https");
const fs = require("fs");
const path = require("path");

const AUTH = "process.env.LINEAR_API_KEY";
const PROJECT_ID = "72e576a7-e894-4e22-b52c-5d47d43a5912";
const API_HOST = "api.linear.app";
const API_PATH = "/graphql";

const SCRIPT_DIR = __dirname;
const COMMENTS_FILE = path.join(SCRIPT_DIR, "linear-comments.json");

function linearPost(payload) {
  return new Promise((resolve, reject) => {
    const body = JSON.stringify(payload);
    const req = https.request(
      {
        hostname: API_HOST,
        path: API_PATH,
        method: "POST",
        headers: {
          Authorization: AUTH,
          "Content-Type": "application/json; charset=utf-8",
          "Content-Length": Buffer.byteLength(body),
        },
      },
      (res) => {
        let data = "";
        res.on("data", (chunk) => (data += chunk));
        res.on("end", () => {
          try {
            resolve(JSON.parse(data));
          } catch (e) {
            reject(new Error(`Parse error: ${data}`));
          }
        });
      }
    );
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

function getComment(title, comments) {
  const rules = [
    [/^RD-M4-0(19|2[0-9]|3[0-9])/, "workspace_domain"],
    [/^RD-M4-0(3[1-9]|4[0-6])/, "file_watcher"],
    [/^QA-M4-/, "qa_m4"],
    [/^GATE-M4/, "gate_m4"],
    [/^RD-M5-00[1-9]|^RD-M5-01[0-4]/, "ui_infra"],
    [/^RD-M5-01[5-9]|^RD-M5-02[0-9]|^RD-M5-030$/, "task_tree"],
    [/^RD-M5-03[1-9]|^RD-M5-040$/, "inspector"],
    [/^RD-M5-04[1-9]|^RD-M5-05[0-2]/, "keyboard"],
    [/^RD-M5-05[3-9]|^RD-M5-06[0-4]/, "filter"],
    [/^QA-M5-/, "qa_m5"],
    [/^GATE-M5/, "gate_m5"],
  ];
  for (const [pattern, key] of rules) {
    if (pattern.test(title)) return comments[key];
  }
  return comments.ui_infra;
}

function isGarbled(body) {
  const firstLine = body.split("\n")[0] || "";

  // Pattern 1: ?? question marks (first sync failure)
  if (/^## \?\? \?\?\?\?\?\?/.test(firstLine)) return true;

  // Pattern 2: Mojibake - has our section structure but garbled H2 header
  const hasSections =
    body.includes("实现方案") ||
    body.includes("遇到的问题") ||
    body.includes("解决方案");
  if (hasSections) {
    // Valid headers: "##  进度更新" or "##  进度更新"
    const validHeader = /^## []?\s*进度更新/.test(firstLine);
    if (!validHeader) return true;
  }

  return false;
}

async function main() {
  // Load comments from UTF-8 JSON file
  const comments = JSON.parse(fs.readFileSync(COMMENTS_FILE, "utf-8"));

  // Step 1: Fetch all Done issues
  console.log("=== Fetching Done issues ===");
  const allIssues = [];
  let after = null;

  while (true) {
    const cursorPart = after ? `, after: "${after}"` : "";
    const resp = await linearPost({
      query: `query { project(id: "${PROJECT_ID}") { issues(first: 100${cursorPart}) { nodes { id identifier title state { name } } pageInfo { hasNextPage endCursor } } } }`,
    });
    allIssues.push(...resp.data.project.issues.nodes);
    const pageInfo = resp.data.project.issues.pageInfo;
    if (!pageInfo.hasNextPage) break;
    after = pageInfo.endCursor;
  }

  // Filter to M4/M5 Done issues
  const doneIssues = allIssues
    .filter(
      (issue) =>
        /^RD-M[45]-|^QA-M[45]-|^GATE-M[45]|^M[45] Epic/.test(issue.title) &&
        issue.state.name === "Done"
    )
    .sort((a, b) => a.identifier.localeCompare(b.identifier));

  console.log(`=== Found ${doneIssues.length} Done tasks ===`);

  // Step 2: Process each issue
  let updated = 0;
  let failed = 0;

  for (const issue of doneIssues) {
    const { id: iid, identifier, title } = issue;
    console.log(`\nProcessing ${identifier}: ${title}`);

    // List existing comments
    const listResp = await linearPost({
      query: "query($id: String!) { issue(id: $id) { comments { nodes { id body createdAt } } } }",
      variables: { id: iid },
    });
    const existing = listResp.data.issue.comments.nodes || [];

    // Delete garbled comments
    let deleted = 0;
    for (const c of existing) {
      if (isGarbled(c.body)) {
        const delResp = await linearPost({
          query: "mutation($id: String!) { commentDelete(id: $id) { success } }",
          variables: { id: c.id },
        });
        if (delResp.data.commentDelete.success) {
          console.log(`  Deleted garbled comment (${c.id})`);
          deleted++;
        }
        await sleep(200);
      }
    }

    // Send correct comment
    const comment = getComment(title, comments);
    const addResp = await linearPost({
      query:
        "mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }",
      variables: { issueId: iid, body: comment },
    });
    if (addResp.data.commentCreate.success) {
      console.log(`  Sent correct comment (${deleted} garbled deleted)`);
      updated++;
    } else {
      console.log("  FAIL: comment create failed");
      failed++;
    }
    await sleep(200);
  }

  console.log(
    `\n=== Fix complete: ${updated} success, ${failed} failed out of ${doneIssues.length} tasks ===`
  );
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
