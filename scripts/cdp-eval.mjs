// Minimal CDP driver for the running Tauri WebView2 (remote-debugging-port 9222).
// Usage:
//   node scripts/cdp-eval.mjs "<js expression>"        -> Runtime.evaluate, prints value
//   node scripts/cdp-eval.mjs --screenshot <path.png>  -> Page.captureScreenshot to file
//   node scripts/cdp-eval.mjs --click "<css selector>" -> dispatch a real mouse click at element center
//   node scripts/cdp-eval.mjs --keys "<key>"           -> Input.dispatchKeyEvent (single key)
const PORT = process.env.CDP_PORT || "9222";
const arg1 = process.argv[2] || "1+1";

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
  await rpc("Page.enable");
  await rpc("DOM.enable");

  const centerOf = async (js) => {
    const box = await rpc("Runtime.evaluate", { expression: js, returnByValue: true });
    if (!box.result.value) return null;
    return JSON.parse(box.result.value);
  };
  const realClick = async (x, y) => {
    for (const type of ["mousePressed", "mouseReleased"]) {
      await rpc("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
    }
  };

  if (arg1 === "--screenshot") {
    const out = process.argv[3] || "screenshot.png";
    const r = await rpc("Page.captureScreenshot", { format: "png" });
    const fs = await import("node:fs");
    fs.writeFileSync(out, Buffer.from(r.data, "base64"));
    console.log("saved " + out);
  } else if (arg1 === "--clicktext") {
    const txt = process.argv[3];
    const js = `(()=>{const el=[...document.querySelectorAll('button,a,[role=button],.palette__item')].find(e=>(e.textContent||'').trim().includes(${JSON.stringify(txt)}));if(!el)return null;const b=el.getBoundingClientRect();return JSON.stringify({x:b.x+b.width/2,y:b.y+b.height/2});})()`;
    const c = await centerOf(js);
    if (!c) throw new Error("text not found: " + txt);
    await realClick(c.x, c.y);
    console.log(`clicked "${txt}" @ ${Math.round(c.x)},${Math.round(c.y)}`);
  } else if (arg1 === "--clicksel") {
    const sel = process.argv[3];
    const c = await centerOf(`(()=>{const el=document.querySelector(${JSON.stringify(sel)});if(!el)return null;const b=el.getBoundingClientRect();return JSON.stringify({x:b.x+b.width/2,y:b.y+b.height/2});})()`);
    if (!c) throw new Error("selector not found: " + sel);
    await realClick(c.x, c.y);
    console.log(`clicked ${sel}`);
  } else if (arg1 === "--type") {
    const txt = process.argv[3] || "";
    await rpc("Input.insertText", { text: txt });
    console.log("typed " + txt.length + " chars");
  } else if (arg1 === "--combo") {
    // e.g. --combo k ctrl  |  --combo Enter
    const key = process.argv[3];
    const mod = process.argv[4] || "";
    const modifiers = mod.includes("ctrl") ? 2 : mod.includes("shift") ? 8 : 0;
    const k = key.length === 1 ? key.toUpperCase() : key;
    const code = key.length === 1 ? "Key" + k : key;
    await rpc("Input.dispatchKeyEvent", { type: "keyDown", key: key.length === 1 ? key : key, code, modifiers, windowsVirtualKeyCode: key.length === 1 ? k.charCodeAt(0) : 0 });
    await rpc("Input.dispatchKeyEvent", { type: "keyUp", key, code, modifiers, windowsVirtualKeyCode: key.length === 1 ? k.charCodeAt(0) : 0 });
    console.log(`sent ${mod}+${key}`);
  } else {
    const r = await rpc("Runtime.evaluate", {
      expression: arg1,
      returnByValue: true,
      awaitPromise: true,
    });
    if (r.exceptionDetails) {
      console.error("EXCEPTION:", JSON.stringify(r.exceptionDetails, null, 2));
    } else {
      console.log(typeof r.result.value === "string" ? r.result.value : JSON.stringify(r.result.value, null, 2));
    }
  }
  ws.close();
  process.exit(0);
}

main().catch((e) => {
  console.error("ERROR:", e.message);
  process.exit(1);
});
