// End-to-end test of the real Tauri backend over CDP (remote-debugging-port 9222).
// Drives: create_workspace -> (ensure dir) -> write doc -> scan_and_index ->
//         list_documents -> open_document_session -> add_task -> save -> query.
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.CDP_PORT || "9222";

async function pageTarget() {
  const r = await fetch(`http://localhost:${PORT}/json/list`);
  const list = await r.json();
  const p = list.find((t) => t.type === "page");
  if (!p) throw new Error("no page target — is the desktop app running?");
  return p;
}
function connect(url) {
  return new Promise((res, rej) => {
    const ws = new WebSocket(url);
    ws.onopen = () => res(ws);
    ws.onerror = (e) => rej(new Error("ws: " + (e.message || e.type)));
  });
}
function makeRpc(ws) {
  const pending = new Map();
  let id = 0;
  ws.addEventListener("message", (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) {
      const { res, rej } = pending.get(m.id);
      pending.delete(m.id);
      m.error ? rej(new Error(JSON.stringify(m.error))) : res(m.result);
    }
  });
  return (method, params = {}) =>
    new Promise((res, rej) => {
      const i = ++id;
      pending.set(i, { res, rej });
      ws.send(JSON.stringify({ id: i, method, params }));
    });
}

const TDL = `<?xml version="1.0" encoding="utf-8"?>\n<TODOLIST PROJECTNAME="" FILENAME="ToDoList.tdl" NEXTUNIQUEID="1" FILEVERSION="43" APPVER="9.0.14.0" FILEFORMAT="12">\n</TODOLIST>\n`;

async function main() {
  const ws = await connect((await pageTarget()).webSocketDebuggerUrl);
  const rpc = makeRpc(ws);
  await rpc("Runtime.enable");
  const inv = async (cmd, args = {}) => {
    const r = await rpc("Runtime.evaluate", {
      expression: `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) throw new Error(cmd + ": " + (r.exceptionDetails.exception?.value || r.exceptionDetails.text));
    return r.result.value;
  };

  const steps = [];
  try {
  const wsRoot = "D:\\Project\\ModernToDoList\\Data\\cdp-e2e-" + Date.now();

  const created = await inv("create_workspace", { path: wsRoot, name: "CDP Test WS" });
  steps.push(["create_workspace", { id: created.id, db_available: created.db_available, root: created.root_path }]);

  // Ensure the folder physically exists (create_workspace may only register it).
  const dirExists = fs.existsSync(wsRoot);
  if (!dirExists) fs.mkdirSync(wsRoot, { recursive: true });
  steps.push(["dir existed?", dirExists]);

  const docPath = path.join(wsRoot, "ToDoList.tdl");
  await inv("serialize_and_write_document", { xmlContent: TDL, path: docPath, encoding: "utf-8" });
  steps.push(["serialize_and_write_document", { fileOnDisk: fs.existsSync(docPath) }]);

  const idx = await inv("scan_and_index");
  steps.push(["scan_and_index", idx]);

  const docs = await inv("list_documents");
  steps.push(["list_documents", docs.map((d) => ({ id: d.id, file_path: d.file_path }))]);

  if (docs.length) {
    const sess = await inv("open_document_session", { path: docs[0].file_path });
    steps.push(["open_document_session", { session_id: sess.session_id }]);
    const add = await inv("add_task", {
      request: {
        session_id: sess.session_id, document_id: docs[0].id, task_key: null,
        title: "写测试用例", parent_key: null, priority: 2, status: "Not Started",
        due_date: null, start_date: null, tags: ["qa"], participants: ["Lamires"],
      },
    });
    steps.push(["add_task", add]);
    await inv("save_document_atomic", { sessionId: sess.session_id });
    const q = await inv("query_tasks", { documentId: docs[0].id });
    steps.push(["query_tasks", (q.tasks || []).map((t) => ({ key: t.task_key, title: t.title, status: t.status }))]);
  }

  console.log(JSON.stringify(steps, null, 2));
  } catch (e) {
    console.log("PARTIAL STEPS:\n" + JSON.stringify(steps, null, 2));
    throw e;
  }
  ws.close();
  process.exit(0);
}
main().catch((e) => { console.error("FAILED:", e.message); process.exit(1); });
