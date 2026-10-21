// M9 UI smoke: Smart Views, Saved Views, shortcut settings via CDP.
const { connectPage } = require('./cdp-client.cjs');
const path = require('path');
const os = require('os');
const fs = require('fs');
const EVID = path.resolve('docs/qa/evidence');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const cdp = await connectPage(9222);
  await cdp.eval(`(async () => {
    window.__e2e = {
      app: await import('/src/stores/app-state.ts'),
      session: await import('/src/stores/session-store.ts'),
      taskStore: await import('/src/stores/task-store.ts'),
    };
  })()`);
  // Open the most recent E2E workspace (has real tasks).
  const tmp = os.tmpdir();
  const ws = fs.readdirSync(tmp).filter(d => d.startsWith('mtdl-e2e-ws-')).sort().pop();
  if (!ws) throw new Error('no e2e workspace found');
  const wsDir = path.join(tmp, ws);
  console.log('[setup] workspace:', wsDir);
  await cdp.eval(`(async () => { const { app } = window.__e2e; await app.openWorkspace(${JSON.stringify(wsDir)}, 'm9-smoke'); await app.scanAndIndex(); })()`);
  await sleep(1500);

  const out = {};
  // Smart Views panel: titles + counts
  out.smartViews = await cdp.eval(`(() => {
    const items = Array.from(document.querySelectorAll('.smart-views__item, [class*="smart-view"]'));
    return items.map(e => e.textContent.trim().replace(/\\s+/g, ' ')).slice(0, 20);
  })()`);
  await cdp.screenshot(path.join(EVID, 'm9-01-smart-views.png'));

  // Click each smart view and record row counts
  out.smartViewRows = await cdp.eval(`(async () => {
    const res = {};
    const items = Array.from(document.querySelectorAll('.smart-views__item'));
    for (const it of items) {
      it.click();
      await new Promise(r => setTimeout(r, 600));
      const label = it.textContent.trim().replace(/\\s+/g, ' ');
      res[label] = document.querySelectorAll('.task-row').length;
    }
    return res;
  })()`);
  await cdp.screenshot(path.join(EVID, 'm9-02-smart-view-click.png'));

  // Saved Views panel presence
  out.savedViews = await cdp.eval(`(() => {
    const el = document.querySelector('.saved-views, [class*="saved-view"]');
    return el ? el.textContent.trim().replace(/\\s+/g, ' ').slice(0, 200) : null;
  })()`);

  // Settings / shortcut section: open settings if present
  out.shortcutSection = await cdp.eval(`(() => {
    const el = document.querySelector('.shortcut-settings, [class*="shortcut"]');
    return el ? el.textContent.trim().replace(/\\s+/g, ' ').slice(0, 200) : null;
  })()`);

  // Quick Add hotkey hint + command palette availability
  out.sidebarText = await cdp.eval(`(() => document.querySelector('.sidebar')?.textContent.replace(/\\s+/g,' ').slice(0, 400) || null)()`);

  console.log(JSON.stringify(out, null, 2));
  cdp.close();
  process.exit(0);
})().catch((e) => { console.error('M9 SMOKE FAIL:', e.message); process.exit(1); });
