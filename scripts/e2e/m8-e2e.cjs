// M8 transfer E2E driver (E1–E6) via CDP against the running debug app.
// Usage: node scripts/e2e/m8-e2e.cjs
// Evidence: screenshots + m8-e2e-results.json under docs/qa/evidence/
const { connectPage } = require('./cdp-client.cjs');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execSync, spawn } = require('child_process');

const WS_DIR = path.join(os.tmpdir(), `mtdl-e2e-ws-${Date.now()}`);
const FIXTURES = path.resolve('tests/fixtures/xml/transfer');
const EVID = path.resolve('docs/qa/evidence');
const APP_EXE = path.resolve('src-tauri/target/debug/ModernToDoList.exe');

const results = [];
function record(id, ok, detail) {
  results.push({ id, ok, detail, at: new Date().toISOString() });
  console.log(`[${ok ? 'PASS' : 'FAIL'}] ${id}: ${detail}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function shot(cdp, name) {
  const p = path.join(EVID, `${name}.png`);
  await cdp.screenshot(p);
  console.log('  [shot]', p);
}

// Evaluate in page context; window.__e2e holds the store singletons.
async function ev(cdp, expr) {
  return cdp.eval(`(async () => { const { app, session, taskStore, transfer } = window.__e2e; ${expr} })()`);
}

async function waitPage(cdp, predExpr, timeoutMs = 15000, label = '') {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const ok = await cdp.eval(predExpr).catch(() => false);
    if (ok) return true;
    await sleep(400);
  }
  throw new Error('waitPage timeout: ' + (label || predExpr));
}

async function docCounts(cdp) {
  return ev(cdp, `
    const out = {};
    for (const d of app.documents.value) {
      await session.ensureSessionForDocument(d.id);
      await taskStore.loadTasks(d.id);
      out[d.file_path || d.path || d.id] = taskStore.allTasks.value.length;
    }
    return out;
  `);
}

async function main() {
  fs.mkdirSync(EVID, { recursive: true });

  // ── Phase 0: workspace with two fixture documents ─────────────────────────
  fs.rmSync(WS_DIR, { recursive: true, force: true });
  fs.mkdirSync(WS_DIR, { recursive: true });
  fs.copyFileSync(path.join(FIXTURES, 'source-project.xml'), path.join(WS_DIR, 'source-project.tdl'));
  fs.copyFileSync(path.join(FIXTURES, 'target-project.xml'), path.join(WS_DIR, 'target-project.tdl'));
  if (fs.existsSync(path.join(FIXTURES, 'assets'))) {
    fs.cpSync(path.join(FIXTURES, 'assets'), path.join(WS_DIR, 'assets'), { recursive: true });
  }
  console.log('[setup] workspace at', WS_DIR);

  let cdp = await connectPage(9222);
  await cdp.eval(`(async () => {
    window.__e2e = {
      app: await import('/src/stores/app-state.ts'),
      session: await import('/src/stores/session-store.ts'),
      taskStore: await import('/src/stores/task-store.ts'),
      transfer: await import('/src/stores/transfer-store.ts'),
    };
  })()`);

  await ev(cdp, `await app.openWorkspace(${JSON.stringify(WS_DIR)}, 'e2e')`);
  await ev(cdp, `await app.scanAndIndex()`);
  const docs = await ev(cdp, `return app.documents.value.map(d => ({ id: d.id, path: d.file_path || d.path, name: d.name }))`);
  console.log('[setup] documents:', JSON.stringify(docs));
  if (!docs || docs.length < 2) throw new Error('expected >=2 documents, got ' + JSON.stringify(docs));
  const srcDoc = docs.find((d) => (d.path || '').includes('source-project'));
  const tgtDoc = docs.find((d) => (d.path || '').includes('target-project'));
  if (!srcDoc || !tgtDoc) throw new Error('fixtures not registered: ' + JSON.stringify(docs));

  // Open source doc in the UI tree
  await ev(cdp, `
    await session.ensureSessionForDocument(${JSON.stringify(srcDoc.id)});
    session.setActiveDocument(${JSON.stringify(srcDoc.id)});
    await taskStore.loadTasks(${JSON.stringify(srcDoc.id)});
  `);
  await waitPage(cdp, `document.querySelectorAll('.task-row').length > 0`, 15000, 'task rows render');
  const baseCounts = await docCounts(cdp);
  console.log('[setup] base counts:', JSON.stringify(baseCounts));
  const srcKey = Object.keys(baseCounts).find((k) => k.includes('source-project'));
  const tgtKey = Object.keys(baseCounts).find((k) => k.includes('target-project'));
  await shot(cdp, 'e2e-00-workspace-open');

  // Helper: right-click first non-group task row and pick a menu entry.
  async function contextMenuOnFirstTask(menuRe, rowRe) {
    await cdp.eval(`(() => {
      const rows = Array.from(document.querySelectorAll('.task-row')).filter(r => !r.className.includes('task-row--group'));
      const el = (${rowRe || 'null'})
        ? rows.find(r => (${rowRe || 'null'}).test(r.textContent))
        : rows[0];
      if (!el) throw new Error('no matching task row');
      const rc = el.getBoundingClientRect();
      el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rc.x + 40, clientY: rc.y + 8 }));
    })()`);
    await waitPage(cdp, `!!document.querySelector('.context-menu')`, 5000, 'context menu');
    const clicked = await cdp.eval(`(() => {
      const items = Array.from(document.querySelectorAll('.context-menu__item'));
      const it = items.find(i => ${menuRe}.test(i.textContent));
      if (!it || it.disabled) return null;
      it.click(); return it.textContent.trim();
    })()`);
    if (!clicked) throw new Error('menu item not found for ' + menuRe);
    console.log('  [menu] clicked:', clicked);
    return clicked;
  }

  // Hold a Windows share-lock on a file for up to `secs` (PowerShell).
  // share='Read' lets the backend READ (preview/plan succeeds) but denies
  // WRITE (execute-time commit fails deterministically).
  // Resolves with { release() } once PowerShell confirms the handle is open;
  // release() kills the process so later phases are not blocked by a stale lock.
  function lockFile(p, share = 'Read', secs = 15) {
    return new Promise((resolve, reject) => {
      const ps = spawn('powershell.exe', [
        '-NoProfile', '-Command',
        `$f=[System.IO.File]::Open('${p}','Open','ReadWrite','${share}'); Write-Output "LOCKED"; Start-Sleep ${secs}; $f.Close()`,
      ], { stdio: ['ignore', 'pipe', 'pipe'] });
      let out = '';
      ps.stdout.on('data', (d) => { out += d; });
      ps.stderr.on('data', (d) => { out += 'STDERR:' + d; });
      ps.on('error', reject);
      const release = () => { try { ps.kill(); } catch {} };
      const t0 = Date.now();
      const iv = setInterval(() => {
        if (out.includes('LOCKED')) { clearInterval(iv); resolve({ release }); }
        else if (Date.now() - t0 > 10000) { clearInterval(iv); reject(new Error('lock timeout: ' + out)); }
      }, 150);
    });
  }

  async function pickTargetAndConfirm(beforeTargetClick, bannerTimeoutMs = 20000, beforeConfirmAccept) {
    try {
      await waitPage(cdp, `!!document.querySelector('.picker-dialog__target')`, 10000, 'target picker');
    } catch (e) {
      const diag = await cdp.eval(`(() => ({
        pickerOverlay: !!document.querySelector('.picker-overlay'),
        pickerText: (document.querySelector('.picker-dialog')||{}).innerText || null,
        contextMenu: !!document.querySelector('.context-menu'),
        toasts: window.__e2e.app.toasts.value.map(t => t.type + ':' + t.message),
        loading: window.__e2e.transfer.transferTargetsLoading.value,
        targets: window.__e2e.transfer.transferTargets.value.map(t => t.display_name + '|w=' + t.writable),
      }))()`).catch((derr) => ({ diagError: derr.message }));
      console.log('  [diag]', JSON.stringify(diag));
      throw e;
    }
    await shot(cdp, 'e2e-picker');
    if (beforeTargetClick) await beforeTargetClick();
    await cdp.eval(`(() => {
      const t = Array.from(document.querySelectorAll('.picker-dialog__target')).find(e => e.textContent.includes('target-project'));
      if (!t) throw new Error('target row not found in picker');
      if (t.disabled) throw new Error('target row disabled (not writable — stale lock?)');
      t.click();
    })()`);
    // preview may raise a warnings confirm dialog; warning-free transfers go
    // straight to the banner. Wait for either instead of a fixed sleep.
    await waitPage(cdp, `!!document.querySelector('.confirm-dialog') || !!document.querySelector('.transfer-banner')`, 10000, 'warnings dialog or banner');
    if (beforeConfirmAccept) await beforeConfirmAccept();
    const hadConfirm = await cdp.eval(`(() => {
      const dlg = document.querySelector('.confirm-dialog');
      if (!dlg) return false;
      dlg.querySelector('.btn-danger').click();
      return true;
    })()`);
    if (hadConfirm) console.log('  [info] warnings confirm dialog accepted');
    await waitPage(cdp, `!!document.querySelector('.transfer-banner')`, bannerTimeoutMs, 'result banner');
  }

  const bannerInfo = `(() => { const b = document.querySelector('.transfer-banner');
    return b ? { cls: b.className, title: (b.querySelector('.transfer-banner__title')||{}).textContent || '', msg: (b.querySelector('.transfer-banner__message')||{}).textContent || '' } : null; })()`;

  async function dismissBanner() {
    await cdp.eval(`(() => { const x = document.querySelector('.transfer-banner__close'); if (x) x.click(); })()`);
    await sleep(300);
  }

  async function reloadSourceView() {
    await ev(cdp, `session.setActiveDocument(${JSON.stringify(srcDoc.id)}); await taskStore.loadTasks(${JSON.stringify(srcDoc.id)})`);
    await sleep(400);
  }

  // ── E1: normal Copy ───────────────────────────────────────────────────────
  try {
    await reloadSourceView();
    await contextMenuOnFirstTask(`/Copy to Document|复制到/`);
    await pickTargetAndConfirm();
    const b = await cdp.eval(bannerInfo);
    console.log('  banner:', JSON.stringify(b));
    const counts = await docCounts(cdp);
    const ok = /Copy completed|复制完成/.test(b.title)
      && counts[tgtKey] > baseCounts[tgtKey]
      && counts[srcKey] === baseCounts[srcKey];
    await shot(cdp, 'e2e-e1-copy-completed');
    record('E1', ok, `copy banner="${b.title}" target ${baseCounts[tgtKey]}->${counts[tgtKey]}, source unchanged=${counts[srcKey] === baseCounts[srcKey]}`);
    // Undo path
    const undoOk = await cdp.eval(`(() => { const u = document.querySelector('.transfer-banner__undo'); if (u) { u.click(); return true; } return false; })()`);
    if (undoOk) {
      await sleep(1500);
      const afterUndo = await docCounts(cdp);
      console.log('  [info] undo: target', counts[tgtKey], '->', afterUndo[tgtKey]);
      record('E1-undo', afterUndo[tgtKey] === baseCounts[tgtKey], `target back to ${afterUndo[tgtKey]}`);
    } else record('E1-undo', false, 'undo button missing');
    await dismissBanner();
    await reloadSourceView();
  } catch (e) { record('E1', false, e.message); await shot(cdp, 'e2e-e1-FAIL'); await dismissBanner(); }

  // ── E2: normal Move (source row removed only after backend confirm) ───────
  try {
    const before = await docCounts(cdp);
    await reloadSourceView();
    await contextMenuOnFirstTask(`/Move to Document|移动到/`);
    await pickTargetAndConfirm();
    const b = await cdp.eval(bannerInfo);
    const counts = await docCounts(cdp);
    const moved = before[srcKey] - counts[srcKey];
    const ok = /Move completed|移动完成|completed/i.test(b.title)
      && moved > 0 && counts[tgtKey] === before[tgtKey] + moved;
    await shot(cdp, 'e2e-e2-move-completed');
    record('E2', ok, `move banner="${b.title}" source ${before[srcKey]}->${counts[srcKey]} target ${before[tgtKey]}->${counts[tgtKey]}`);
    await dismissBanner();
    await reloadSourceView();
  } catch (e) { record('E2', false, e.message); await shot(cdp, 'e2e-e2-FAIL'); await dismissBanner(); }

  // ── E3: target failure (target share-locked between preview and execute) ──
  let e3lock = null;
  try {
    const before = await docCounts(cdp);
    await reloadSourceView();
    await contextMenuOnFirstTask(`/Copy to Document|复制到/`);
    await pickTargetAndConfirm(async () => {
      e3lock = await lockFile(path.join(WS_DIR, 'target-project.tdl'));
    });
    const b = await cdp.eval(bannerInfo);
    const counts = await docCounts(cdp);
    const ok = /failed before the target|目标提交前失败/i.test(b.title + b.msg)
      && counts[srcKey] === before[srcKey] && counts[tgtKey] === before[tgtKey];
    await shot(cdp, 'e2e-e3-target-failed');
    record('E3', ok, `banner="${b.title}" counts unchanged=${counts[srcKey] === before[srcKey] && counts[tgtKey] === before[tgtKey]}`);
    await dismissBanner();
    await reloadSourceView();
  } catch (e) {
    record('E3', false, e.message); await shot(cdp, 'e2e-e3-FAIL');
    await dismissBanner();
  } finally {
    // Release promptly: a stale target lock makes the NEXT picker's target
    // read-only (row disabled) and silently blocks E4.
    if (e3lock) e3lock.release();
  }

  // ── E4: source-commit failure (source write-locked between confirm and
  //    execute) → target commits, guarded source delete fails → Recovery
  //    Required; BOTH documents keep the subtree (duplicate-not-loss). ──
  let e4lock = null;
  try {
    const srcPath = path.join(WS_DIR, 'source-project.tdl');
    const before = await docCounts(cdp);
    await reloadSourceView();
    // "Release Epic" subtree carries an external dependency (task 6 → task 1),
    // so its preview always shows the warnings dialog — our injection window.
    await contextMenuOnFirstTask(`/Move to Document|移动到/`, `/Release Epic/`);
    await pickTargetAndConfirm(null, 30000, async () => {
      e4lock = await lockFile(srcPath);
      console.log('  [info] source file write-locked between confirm and execute');
    });
    const b = await cdp.eval(bannerInfo);
    const counts = await docCounts(cdp);
    const ok = /Recovery required|需要恢复|Move incomplete|移动未完成/i.test(b.title)
      && counts[srcKey] >= before[srcKey] /* source not lost */
      && counts[tgtKey] > before[tgtKey] /* duplicate preserved */;
    await shot(cdp, 'e2e-e4-recovery-required');
    record('E4', ok, `banner="${b.title}" source ${before[srcKey]}->${counts[srcKey]} target ${before[tgtKey]}->${counts[tgtKey]}`);
    await dismissBanner();
  } catch (e) { record('E4', false, e.message); await shot(cdp, 'e2e-e4-FAIL'); await dismissBanner(); }
  finally {
    // Release before E5: the recovery "Complete Move" step writes the source.
    if (e4lock) e4lock.release();
  }

  // ── E5: kill app mid-recovery state, restart, recovery panel actions ──────
  try {
    try { execSync('taskkill /IM ModernToDoList.exe /F'); } catch {}
    await sleep(1500);
    const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9222' };
    const child = spawn(APP_EXE, [], { env, detached: true, stdio: 'ignore' });
    child.unref();
    // wait for CDP to come back
    let up = false;
    for (let i = 0; i < 40; i++) {
      try { cdp = await connectPage(9222); up = true; break; } catch { await sleep(500); }
    }
    if (!up) throw new Error('app did not come back on :9222');
    await waitPage(cdp, `!!document.querySelector('.app-root')`, 30000, 'app render after restart');
    await cdp.eval(`(async () => {
      window.__e2e = {
        app: await import('/src/stores/app-state.ts'),
        session: await import('/src/stores/session-store.ts'),
        taskStore: await import('/src/stores/task-store.ts'),
        transfer: await import('/src/stores/transfer-store.ts'),
      };
    })()`);
    await ev(cdp, `await app.loadWorkspace(${JSON.stringify(WS_DIR)})`);
    // watch(workspace) triggers runRecoveryScan; wait for panel
    let hasPanel = false;
    try { await waitPage(cdp, `!!document.querySelector('.recovery-panel__item')`, 20000, 'recovery panel'); hasPanel = true; } catch {}
    if (!hasPanel) {
      const reports = await ev(cdp, `return transfer.recoveryReports.value`);
      throw new Error('no recovery panel; reports=' + JSON.stringify(reports));
    }
    const e5reports = await ev(cdp, `return transfer.recoveryReports.value.map(r => r.action)`);
    console.log('  [info] recovery reports:', JSON.stringify(e5reports));
    if (!e5reports.includes('completed_with_duplicate')) {
      throw new Error('expected completed_with_duplicate report, got ' + JSON.stringify(e5reports));
    }
    await shot(cdp, 'e2e-e5-recovery-panel');
    const before = await docCounts(cdp);
    // Path A: Complete Move
    await cdp.eval(`(() => {
      const btns = Array.from(document.querySelectorAll('.recovery-panel__actions .btn'));
      const b = btns.find(x => /Complete Move|完成移动/.test(x.textContent));
      if (b) b.click();
    })()`);
    await sleep(800);
    await cdp.eval(`(() => { const d = document.querySelector('.confirm-dialog .btn-danger'); if (d) d.click(); })()`);
    await sleep(2500);
    const e5diag = await cdp.eval(`(() => ({
      toasts: window.__e2e.app.toasts.value.map(t => t.type + ':' + t.message),
      reports: window.__e2e.transfer.recoveryReports.value.map(r => r.action + '|' + r.detail.slice(0, 80)),
    }))()`).catch((e) => ({ diagError: e.message }));
    console.log('  [diag-e5]', JSON.stringify(e5diag));
    const afterA = await docCounts(cdp);
    const panelLeftA = await cdp.eval(`document.querySelectorAll('.recovery-panel__item').length`);
    await shot(cdp, 'e2e-e5-after-complete');
    record('E5-complete', panelLeftA === 0 && afterA[srcKey] <= before[srcKey],
      `complete-move: panel items ${panelLeftA}, source ${before[srcKey]}->${afterA[srcKey]}, target ${before[tgtKey]}->${afterA[tgtKey]}`);
  } catch (e) { record('E5', false, e.message); try { await shot(cdp, 'e2e-e5-FAIL'); } catch {} }

  // ── E6: drag to sidebar document, cancel → no side effects ───────────────
  try {
    await ev(cdp, `
      await session.ensureSessionForDocument(${JSON.stringify(srcDoc.id)});
      session.setActiveDocument(${JSON.stringify(srcDoc.id)});
      await taskStore.loadTasks(${JSON.stringify(srcDoc.id)});
    `);
    await sleep(500);
    const before = await docCounts(cdp);
    // docCounts leaves the target doc's rows on screen — switch back to source
    // and wait until the rendered rows actually belong to the source document.
    await ev(cdp, `session.setActiveDocument(${JSON.stringify(srcDoc.id)}); await taskStore.loadTasks(${JSON.stringify(srcDoc.id)})`);
    await waitPage(cdp, `(() => { const r = document.querySelector('.task-row:not(.task-row--group)'); return r && /Unrelated Root|Release Epic|Stay Behind|Build Installer|Sign Binaries/.test(r.textContent); })()`, 8000, 'source rows rendered');
    await sleep(300);
    const dropped = await cdp.eval(`(() => {
      const rows = Array.from(document.querySelectorAll('.task-row')).filter(r => !r.className.includes('task-row--group'));
      const row = rows[0];
      const item = Array.from(document.querySelectorAll('.sidebar__item')).find(e => e.textContent.includes('target-project'));
      if (!row || !item) return 'missing:' + (row ? '' : 'row ') + (item ? '' : 'item');
      const dt = new DataTransfer();
      row.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
      item.dispatchEvent(new DragEvent('dragover', { bubbles: true, dataTransfer: dt, cancelable: true }));
      item.dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: dt, cancelable: true }));
      return 'ok';
    })()`);
    if (dropped !== 'ok') throw new Error('drag dispatch: ' + dropped);
    let confirmVisible = false;
    try { await waitPage(cdp, `!!document.querySelector('.confirm-overlay')`, 6000, 'drop confirm'); confirmVisible = true; } catch {}
    if (confirmVisible) {
      await cdp.eval(`(() => {
        const dlg = document.querySelector('.confirm-dialog');
        const btns = Array.from(dlg.querySelectorAll('.btn'));
        const cancel = btns.find(b => !b.className.includes('btn-danger'));
        cancel.click();
      })()`);
    }
    await sleep(1000);
    const counts = await docCounts(cdp);
    const noBanner = await cdp.eval(`!document.querySelector('.transfer-banner')`);
    const ok = confirmVisible && noBanner
      && counts[srcKey] === before[srcKey] && counts[tgtKey] === before[tgtKey];
    await shot(cdp, 'e2e-e6-drop-cancel');
    record('E6', ok, `confirmShown=${confirmVisible} noBanner=${noBanner} counts unchanged=${counts[srcKey] === before[srcKey] && counts[tgtKey] === before[tgtKey]}`);
  } catch (e) { record('E6', false, e.message); await shot(cdp, 'e2e-e6-FAIL'); }

  fs.writeFileSync(path.join(EVID, 'm8-e2e-results.json'), JSON.stringify(results, null, 2));
  const failed = results.filter((r) => !r.ok);
  console.log(`\n== E2E SUMMARY: ${results.length - failed.length}/${results.length} passed ==`);
  cdp.close();
  process.exit(failed.length ? 1 : 0);
}

main().catch((e) => { console.error('E2E DRIVER FATAL:', e); process.exit(2); });
