// Seed a realistic workspace on disk via the real backend, then close it so the
// UI can open it fresh through the native folder dialog.
import fs from "node:fs";
import path from "node:path";
const PORT = "9222";
const p = (await (await fetch(`http://localhost:${PORT}/json/list`)).json()).find(t=>t.type==="page");
const ws = await new Promise((res,rej)=>{const w=new WebSocket(p.webSocketDebuggerUrl);w.onopen=()=>res(w);w.onerror=rej;});
let id=0; const pend=new Map();
ws.addEventListener("message",e=>{const m=JSON.parse(e.data);if(m.id&&pend.has(m.id)){const{r,j}=pend.get(m.id);pend.delete(m.id);m.error?j(new Error(JSON.stringify(m.error))):r(m.result);}});
const rpc=(method,params={})=>new Promise((r,j)=>{const i=++id;pend.set(i,{r,j});ws.send(JSON.stringify({id:i,method,params}));});
await rpc("Runtime.enable");
const inv=async(cmd,args={})=>{const x=await rpc("Runtime.evaluate",{expression:`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},${JSON.stringify(args)})`,awaitPromise:true,returnByValue:true});if(x.exceptionDetails)throw new Error(cmd+": "+(x.exceptionDetails.exception?.value||x.exceptionDetails.text));return x.result.value;};

const root = "D:\\Project\\ModernToDoList\\Data\\wf-" + Date.now();
fs.mkdirSync(root,{recursive:true});
const TDL = `<?xml version="1.0" encoding="utf-8"?>\n<TODOLIST PROJECTNAME="发布计划" FILENAME="ToDoList.tdl" NEXTUNIQUEID="1" FILEVERSION="43" APPVER="9.0.14.0" FILEFORMAT="12">\n</TODOLIST>\n`;
await inv("create_workspace",{path:root,name:"发布计划"});
const docPath=path.join(root,"ToDoList.tdl");
await inv("serialize_and_write_document",{xmlContent:TDL,path:docPath,encoding:"utf-8"});
await inv("scan_and_index");
const docs=await inv("list_documents");
const s=await inv("open_document_session",{path:docPath});
const sid=s.session_id, did=docs[0].id;
const add=async(title,parent,prio,status,tags,parts)=>inv("add_task",{request:{sessionId:sid,session_id:sid,document_id:did,task_key:null,title,parent_key:parent,priority:prio,status,due_date:null,start_date:null,tags,participants:parts}});
const r1=await add("发布 v2.0",null,1,"In Progress",["release","p0"],["Lamires"]);
const r2=await add("编写迁移脚本",r1.task_key,2,"Not Started",["db"],[]);
const r3=await add("补充测试用例",r1.task_key,3,"Not Started",["qa"],[]);
const r4=await add("打包便携版",r1.task_key,2,"Done",["build"],[]);
await inv("add_dependency",{sessionId:sid,taskKey:r4.task_key,documentId:did,dependsOnKey:r2.task_key,dependsOnDocumentId:null,depType:0});
await inv("save_document_atomic",{sessionId:sid});
await inv("close_document_session",{sessionId:sid});
await inv("close_workspace");
console.log(JSON.stringify({root, tasks:[r1.task_key,r2.task_key,r3.task_key,r4.task_key]}));
ws.close(); process.exit(0);
