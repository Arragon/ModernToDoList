// Desktop full functional test over CDP (remote-debugging-port 9222).
// Exercises the real Tauri backend across every IPC command group, isolating
// each case so one failure does not abort the run. Emits JSON results.
import fs from "node:fs";
import path from "node:path";

const PORT = process.env.CDP_PORT || "9222";

async function pageTarget() {
  const r = await fetch(`http://localhost:${PORT}/json/list`);
  const p = (await r.json()).find((t) => t.type === "page");
  if (!p) throw new Error("no page target");
  return p;
}
function connect(url) {
  return new Promise((res, rej) => {
    const ws = new WebSocket(url);
    ws.onopen = () => res(ws);
    ws.onerror = (e) => rej(new Error("ws " + (e.message || e.type)));
  });
}
function makeRpc(ws) {
  const pending = new Map(); let id = 0;
  ws.addEventListener("message", (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) { const { res, rej } = pending.get(m.id); pending.delete(m.id); m.error ? rej(new Error(JSON.stringify(m.error))) : res(m.result); }
  });
  return (method, params = {}) => new Promise((res, rej) => { const i = ++id; pending.set(i, { res, rej }); ws.send(JSON.stringify({ id: i, method, params })); });
}

const results = [];
function record(group, cmd, ok, detail) { results.push({ group, cmd, ok, detail }); }

async function main() {
  const ws = await connect((await pageTarget()).webSocketDebuggerUrl);
  const rpc = makeRpc(ws);
  await rpc("Runtime.enable");
  const inv = async (cmd, args = {}) => {
    const r = await rpc("Runtime.evaluate", {
      expression: `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`,
      awaitPromise: true, returnByValue: true,
    });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.value || r.exceptionDetails.text || "ipc error");
    return r.result.value;
  };
  // test one command, capture pass/fail
  const T = async (group, cmd, fn, summarize) => {
    try { const v = await fn(); record(group, cmd, true, summarize ? summarize(v) : brief(v)); }
    catch (e) { record(group, cmd, false, String(e.message || e)); }
  };
  const brief = (v) => { const s = JSON.stringify(v); return s && s.length > 200 ? s.slice(0, 200) + "…" : s; };

  const root = "D:\\Project\\ModernToDoList\\Data\\ft-" + Date.now();
  fs.mkdirSync(root, { recursive: true });
  const TDL = `<?xml version="1.0" encoding="utf-8"?>\n<TODOLIST PROJECTNAME="" FILENAME="ToDoList.tdl" NEXTUNIQUEID="1" FILEVERSION="43" APPVER="9.0.14.0" FILEFORMAT="12">\n</TODOLIST>\n`;
  const docPath = path.join(root, "ToDoList.tdl");
  let docId = null, sessionId = null, taskA = null, taskB = null, linkId = null, attachId = null, viewId = null;

  // ---- M1 System ----
  await T("M1 System", "ping", () => inv("ping"));
  await T("M1 System", "get_runtime_info", () => inv("get_runtime_info"));

  // ---- M4 Workspace ----
  await T("M4 Workspace", "create_workspace", () => inv("create_workspace", { path: root, name: "FullTest" }));
  await T("M4 Workspace", "get_workspace_status", () => inv("get_workspace_status"));
  await T("M4 Workspace", "write doc (serialize_and_write_document)", () => inv("serialize_and_write_document", { xmlContent: TDL, path: docPath, encoding: "utf-8" }));
  await T("M4 Workspace", "scan_and_index", () => inv("scan_and_index"));
  await T("M4 Workspace", "list_documents", async () => { const d = await inv("list_documents"); if (d[0]) docId = d[0].id; return d.map((x) => ({ id: x.id, file_path: x.file_path })); });
  await T("M4 Workspace", "get_db_status", () => inv("get_db_status"));

  // ---- M2 Document ----
  await T("M2 Document", "get_document_metadata", () => inv("get_document_metadata", { path: docPath }));
  await T("M2 Document", "validate_document_cmd", () => inv("validate_document_cmd", { path: docPath }));
  await T("M2 Document", "read_and_parse_document", () => inv("read_and_parse_document", { path: docPath }));
  await T("M2 Document", "allocate_task_id", () => inv("allocate_task_id", { existingIds: [], nextUniqueId: 1 }));

  // ---- M3 Session ----
  await T("M3 Session", "open_document_session (ABSOLUTE path)", async () => { const s = await inv("open_document_session", { path: docPath }); sessionId = s.session_id; return s; });
  await T("M3 Session", "open_document_session (RELATIVE path, as UI passes)", () => inv("open_document_session", { path: "ToDoList.tdl" }));
  await T("M3 Session", "get_session_status", () => inv("get_session_status", { sessionId }));

  // ---- M6 Task mutation ----
  await T("M6 Task", "add_task A", async () => { const r = await inv("add_task", { request: { sessionId, session_id: sessionId, document_id: docId, task_key: null, title: "父任务", parent_key: null, priority: 2, status: "Not Started", due_date: null, start_date: null, tags: ["p0"], participants: [] } }); taskA = r.task_key; return r; });
  await T("M6 Task", "add_task B (child of A)", async () => { const r = await inv("add_task", { request: { sessionId, session_id: sessionId, document_id: docId, task_key: null, title: "子任务", parent_key: taskA, priority: 3, status: "Not Started", due_date: null, start_date: null, tags: [], participants: [] } }); taskB = r.task_key; return r; });
  await T("M6 Task", "update_task_field title", () => inv("update_task_field", { sessionId, taskKey: taskA, field: "name", value: "父任务(改名)" }));
  await T("M6 Task", "update_task_field priority", () => inv("update_task_field", { sessionId, taskKey: taskA, field: "priority", value: "1" }));
  await T("M6 Task", "update_task_field status=Done", () => inv("update_task_field", { sessionId, taskKey: taskB, field: "status", value: "Done" }));
  await T("M6 Task", "update_task_field due_date", () => inv("update_task_field", { sessionId, taskKey: taskA, field: "duedate", value: "2026-12-31" }));
  await T("M6 Task", "set_task_tags", () => inv("set_task_tags", { sessionId, taskKey: taskA, documentId: docId, tags: ["alpha", "beta"] }));
  await T("M6 Task", "undo_last_command", () => inv("undo_last_command", { sessionId }));
  await T("M6 Task", "redo_last_command", () => inv("redo_last_command", { sessionId }));
  await T("M6 Task", "save_document_atomic", () => inv("save_document_atomic", { sessionId }));

  // ---- M5 Task query ----
  await T("M5 Query", "query_tasks", async () => { const q = await inv("query_tasks", { documentId: docId }); return (q.tasks || []).map((t) => ({ key: t.task_key, title: t.title, status: t.status, prio: t.priority })); });
  await T("M5 Query", "get_task_tags", () => inv("get_task_tags", { taskKey: taskA }));

  // ---- rebuild index (percent_done bug probe) ----
  await T("M4 Workspace", "rebuild_index (after save)", () => inv("rebuild_index"));
  await T("M5 Query", "query_tasks (after rebuild)", async () => { const q = await inv("query_tasks", { documentId: docId }); return (q.tasks || []).map((t) => t.title); });

  // ---- M6 Participants ----
  await T("M6 Participant", "add_participant", () => inv("add_participant", { sessionId, taskKey: taskA, documentId: docId, displayName: "Lamires", role: "allocated_to" }));
  await T("M6 Participant", "get_participants", () => inv("get_participants", { taskKey: taskA, documentId: docId }));
  await T("M6 Participant", "list_participants", () => inv("list_participants", { documentId: docId }));
  await T("M6 Participant", "list_task_participants", () => inv("list_task_participants", { documentId: docId }));
  await T("M6 Participant", "remove_participant", () => inv("remove_participant", { sessionId, taskKey: taskA, documentId: docId, displayName: "Lamires", role: "allocated_to" }));

  // ---- M6 Dependencies ----
  await T("M6 Dependency", "add_dependency", () => inv("add_dependency", { sessionId, taskKey: taskA, documentId: docId, dependsOnKey: taskB, dependsOnDocumentId: null, depType: 0 }));
  await T("M6 Dependency", "get_dependencies", () => inv("get_dependencies", { taskKey: taskA, documentId: docId }));
  await T("M6 Dependency", "list_dependencies", () => inv("list_dependencies", { documentId: docId }));
  await T("M6 Dependency", "remove_dependency", () => inv("remove_dependency", { sessionId, taskKey: taskA, documentId: docId, dependsOnKey: taskB }));

  // ---- M6 Progress links ----
  await T("M6 ProgressLink", "add_progress_link", async () => { const r = await inv("add_progress_link", { sessionId, taskKey: taskA, documentId: docId, label: "PR-1", url: "https://example.com/pr/1", provider: "github" }); linkId = r.link_id ?? r.id; return r; });
  await T("M6 ProgressLink", "list_progress_links", () => inv("list_progress_links", { taskKey: taskA, documentId: docId }));
  await T("M6 ProgressLink", "update_progress_link", () => inv("update_progress_link", { linkId, taskKey: taskA, documentId: docId, label: "PR-1-updated", url: "https://example.com/pr/2", provider: "github" }));
  await T("M6 ProgressLink", "remove_progress_link", () => inv("remove_progress_link", { linkId, taskKey: taskA, documentId: docId }));

  // ---- M6 Attachments ----
  await T("M6 Attachment", "add_url_attachment", async () => { const r = await inv("add_url_attachment", { sessionId, taskKey: taskA, documentId: docId, url: "https://example.com/doc", displayName: "外链文档" }); attachId = r.attachment_id ?? r.id ?? (r.attachments && r.attachments[0] && r.attachments[0].id); return r; });
  await T("M6 Attachment", "list_attachments", () => inv("list_attachments", { taskKey: taskA, documentId: docId }));
  await T("M6 Attachment", "add_managed_attachment (real file)", async () => { const src = path.join(root, "note.txt"); fs.writeFileSync(src, "hello attachment"); return inv("add_managed_attachment", { sessionId, taskKey: taskB, documentId: docId, sourcePath: src, displayName: "note.txt" }); });
  await T("M6 Attachment", "link_local_attachment (real file)", async () => { const src = path.join(root, "local.bin"); fs.writeFileSync(src, "localdata"); return inv("link_local_attachment", { sessionId, taskKey: taskB, documentId: docId, sourcePath: src, displayName: "local.bin" }); });
  await T("M6 Attachment", "update_attachment", () => inv("update_attachment", { sessionId, attachmentId: attachId, taskKey: taskA, documentId: docId, displayName: "外链文档2" }));
  await T("M6 Attachment", "reveal_attachment", () => inv("reveal_attachment", { attachmentId: attachId, taskKey: taskA, documentId: docId }));
  await T("M6 Attachment", "remove_attachment", () => inv("remove_attachment", { sessionId, attachmentId: attachId, taskKey: taskA, documentId: docId }));

  // ---- M7 Comments ----
  await T("M7 Comments", "get_task_comments", () => inv("get_task_comments", { sessionId, taskKey: taskA }));

  // ---- M9 Search / views / quick add ----
  await T("M9 Search", "global_search '父'", () => inv("global_search", { query: "父", documentId: docId, limit: 50, offset: 0 }));
  await T("M9 Search", "global_search CJK '子任务'", () => inv("global_search", { query: "子任务", limit: 50, offset: 0 }));
  await T("M9 View", "create_saved_view", async () => { const wsId = (await inv("get_workspace_status")).id; const r = await inv("create_saved_view", { workspaceId: wsId, name: "高优先级", predicates: { type: "predicate", field: "priority", op: "lte", value: "2" } }); viewId = r.id ?? r.view_id; return r; });
  await T("M9 View", "list_saved_views", async () => { const wsId = (await inv("get_workspace_status")).id; return inv("list_saved_views", { workspaceId: wsId }); });
  await T("M9 View", "rename_saved_view", () => inv("rename_saved_view", { viewId, name: "高优先级(改名)" }));
  await T("M9 View", "delete_saved_view", () => inv("delete_saved_view", { viewId }));
  await T("M9 QuickAdd", "quick_add_task", () => inv("quick_add_task", { request: { text: "快速任务 #urgent !1 @home", documentId: docId } }));

  // ---- delete task (do near end) ----
  await T("M6 Task", "delete_task B", () => inv("delete_task", { sessionId, taskKey: taskB }));
  await T("M6 Task", "delete_task A (has children/relations)", () => inv("delete_task", { sessionId, taskKey: taskA }));

  // ---- session close / workspace lifecycle ----
  await T("M3 Session", "close_document_session", () => inv("close_document_session", { sessionId }));
  await T("M4 Workspace", "close_workspace", () => inv("close_workspace"));
  await T("M4 Workspace", "open_workspace (reopen)", () => inv("open_workspace", { path: root }));

  console.log(JSON.stringify({ root, docId, sessionId, taskA, taskB, results }, null, 2));
  ws.close();
  process.exit(0);
}
main().catch((e) => { console.log(JSON.stringify({ fatal: e.message, results }, null, 2)); process.exit(1); });
