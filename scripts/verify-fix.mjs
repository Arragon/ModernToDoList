// Verifies BUG-5 (cross-workspace task leakage) and the new-task visibility fix
// against the REAL running desktop backend via CDP (port 9222).
//
// Sequence:
//   1. open wf-retest-tasks (2 tasks) -> scan_and_index -> query_tasks({}) == 2
//   2. open wf-retest-empty (0 tasks) -> scan_and_index -> query_tasks({}) == 0
//      PASS(BUG-5) iff step 2 returns 0 (no leakage of step 1's tasks).
//   3. create a task in wf-retest-empty via the same flow the UI "+" uses, then
//      query_tasks({}) again -> must include the new task (visibility fix).
const PORT = process.env.CDP_PORT || "9222";
const ROOT = "D:/Project/ModernToDoList/Data";

async function pageTarget() {
  const res = await fetch(`http://localhost:${PORT}/json/list`);
  const list = await res.json();
  const page = list.find((t) => t.type === "page");
  if (!page) throw new Error("no page target");
  return page;
}
function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.onopen = () => resolve(ws);
    ws.onerror = (e) => reject(new Error("ws error: " + (e.message || e.type)));
  });
}
let seq = 0;
function makeRpc(ws) {
  const pending = new Map();
  ws.addEventListener("message", (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  });
  return (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });
}

async function main() {
  const target = await pageTarget();
  const ws = await connect(target.webSocketDebuggerUrl);
  const rpc = makeRpc(ws);
  await rpc("Runtime.enable");

  // Run an async JS expression in the page and return its JSON value.
  const run = async (expr) => {
    const r = await rpc("Runtime.evaluate", {
      expression: expr,
      returnByValue: true,
      awaitPromise: true,
    });
    if (r.exceptionDetails) {
      throw new Error("page exception: " + JSON.stringify(r.exceptionDetails.exception?.description || r.exceptionDetails.text));
    }
    return r.result.value;
  };

  const inv = (cmd, args = {}) =>
    run(`(async()=>{const r=await window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)});return JSON.stringify(r);})()`);

  const results = [];
  const log = (label, ok, detail) => {
    results.push({ label, ok, detail });
    console.log(`${ok ? "PASS" : "FAIL"} | ${label} | ${detail}`);
  };

  // --- Step 0: fresh state ---
  const st0 = JSON.parse(await inv("get_workspace_status"));
  log("initial workspace status is null", st0 === null, JSON.stringify(st0));

  // --- Step 1: open workspace A (2 tasks) ---
  const wsA = JSON.parse(await inv("open_workspace", { path: `${ROOT}/wf-retest-tasks` }));
  const idxA = JSON.parse(await inv("scan_and_index"));
  const docsA = JSON.parse(await inv("list_documents"));
  const qA = JSON.parse(await inv("query_tasks", {}));
  log("A: open+scan indexes 2 tasks", idxA.total_tasks === 2, `total_tasks=${idxA.total_tasks}`);
  log("A: query_tasks({}) returns 2", qA.total === 2 && qA.tasks.length === 2, `total=${qA.total} titles=${qA.tasks.map(t => t.title).join(",")}`);
  const docIdsA = docsA.map(d => d.id);
  log("A: percent_done coercion (0 & 50)", qA.tasks.some(t => t.percent_done === 0) && qA.tasks.some(t => t.percent_done === 50), `percents=${qA.tasks.map(t => t.percent_done).join(",")}`);
  log("A: list_documents returns absolute paths", docsA.every(d => d.file_path.includes("wf-retest-tasks")), JSON.stringify(docsA.map(d => d.file_path)));

  // --- Step 2: open workspace B (0 tasks) — the BUG-5 leakage probe ---
  const wsB = JSON.parse(await inv("open_workspace", { path: `${ROOT}/wf-retest-empty` }));
  const idxB = JSON.parse(await inv("scan_and_index"));
  const docsB = JSON.parse(await inv("list_documents"));
  const qB = JSON.parse(await inv("query_tasks", {}));
  const docIdsB = docsB.map(d => d.id);
  const leaked = qB.tasks.filter(t => docIdsA.includes(t.document_id));
  log("BUG-5: B query_tasks({}) returns 0 (no leak from A)", qB.total === 0 && leaked.length === 0, `total=${qB.total} leakedFromA=${leaked.length} leakedTitles=${leaked.map(t => t.title).join(",")}`);
  log("B: all returned tasks belong to B's docs", qB.tasks.every(t => docIdsB.includes(t.document_id)), `Bdocs=${docIdsB.length} returnedDocs=${[...new Set(qB.tasks.map(t => t.document_id))].length}`);

  // --- Step 3: new-task visibility (simulate the UI "+" bootstrap+create flow) ---
  // Allocate a key and create a task in B's first document, then re-query.
  const docB = docsB[0];
  if (docB) {
    const alloc = JSON.parse(await inv("allocate_task_key", { documentId: docB.id }));
    const created = JSON.parse(await inv("create_task", {
      title: "复测新建任务-visibility",
      documentId: docB.id,
      parentKey: null,
      priority: 0,
      tags: [],
      taskKey: alloc.task_key ?? alloc.taskKey ?? null,
    }));
    const qB2 = JSON.parse(await inv("query_tasks", {}));
    const found = qB2.tasks.find(t => (t.title || "").includes("visibility"));
    log("visibility: new task appears in query_tasks({}) immediately", !!found, `total=${qB2.total} found=${found ? found.title : "NONE"} created=${JSON.stringify(created).slice(0, 80)}`);
  } else {
    log("visibility: B has a document to create into", false, "no documents in B");
  }

  const pass = results.filter(r => r.ok).length;
  console.log(`\n=== ${pass}/${results.length} checks passed ===`);
  if (pass !== results.length) {
    console.log("FAILED CHECKS:");
    results.filter(r => !r.ok).forEach(r => console.log("  - " + r.label + " :: " + r.detail));
  }
  ws.close();
  process.exit(pass === results.length ? 0 : 1);
}

main().catch((e) => {
  console.error("ERROR:", e.message);
  process.exit(2);
});
