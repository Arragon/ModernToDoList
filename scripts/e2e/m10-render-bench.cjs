// M10 render benchmark (RD-M10-022 decision gate) via CDP against the running debug app.
// Measures rAF frame times while programmatically scrolling the task tree at 10k rows,
// with virtualization OFF and ON. Gate: p95 frame time > 16 ms at 10k+ visible rows
// → virtualization IMPLEMENT (docs/performance/PROFILING_METHOD.md §4.2).
// Usage: node scripts/e2e/m10-render-bench.cjs [--rows 10000]
// Output: docs/qa/evidence/m10-render-bench.json
//
// DEV-SERVER ONLY: this harness injects `window.__e2e` via dynamic imports of
// `/src/stores/*.ts`, which only resolve against `vite dev`. It cannot drive the
// bundled frontend of the release EXE — see docs/qa/LEFTOVER_TASKS.md L-02.
const { connectPage } = require('./cdp-client.cjs');
const fs = require('fs');
const path = require('path');

const EVID = path.resolve('docs/qa/evidence');
const DATASET_DIR = path.resolve('Data/bench/2026-10-21T04-58-33-039Z/datasets/scale-10k');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function ev(cdp, expr) {
  return cdp.eval(`(async () => { const { app, session, taskStore, settingsStore } = window.__e2e; ${expr} })()`);
}

async function waitPage(cdp, predExpr, timeoutMs = 15000, label = '') {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const ok = await cdp.eval(predExpr).catch(() => false);
    if (ok) return true;
    await sleep(500);
  }
  throw new Error('waitPage timeout: ' + (label || predExpr));
}

// Scroll the tree scroller for `durationMs` while sampling rAF deltas in-page.
// Returns the raw delta array (ms). Scrolling is driven inside the same rAF
// loop so every sampled frame carries real layout/paint work.
const MEASURE_EXPR = (durationMs) => `
  const scroller = document.querySelector('.task-tree__scroller');
  if (!scroller) throw new Error('no scroller');
  return await new Promise((resolve) => {
    const deltas = [];
    let last = performance.now();
    const t0 = last;
    let dir = 1;
    function frame(now) {
      const d = now - last;
      last = now;
      deltas.push(d);
      const maxTop = scroller.scrollHeight - scroller.clientHeight;
      let next = scroller.scrollTop + dir * 240;
      if (next >= maxTop) { next = maxTop; dir = -1; }
      if (next <= 0) { next = 0; dir = 1; }
      scroller.scrollTop = next;
      if (now - t0 < ${durationMs}) requestAnimationFrame(frame);
      else resolve(deltas);
    }
    requestAnimationFrame(frame);
  });
`;

function percentile(sorted, p) {
  if (!sorted.length) return null;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

function summarize(deltas) {
  const s = [...deltas].sort((a, b) => a - b);
  const sum = deltas.reduce((a, b) => a + b, 0);
  return {
    frames: deltas.length,
    mean_ms: +(sum / deltas.length).toFixed(2),
    p50_ms: +percentile(s, 50).toFixed(2),
    p95_ms: +percentile(s, 95).toFixed(2),
    p99_ms: +percentile(s, 99).toFixed(2),
    max_ms: +s[s.length - 1].toFixed(2),
    fps_effective: +(1000 / (sum / deltas.length)).toFixed(1),
  };
}

async function main() {
  fs.mkdirSync(EVID, { recursive: true });
  if (!fs.existsSync(path.join(DATASET_DIR, 'scale-10k.xml'))) {
    throw new Error('dataset missing: ' + DATASET_DIR);
  }

  const cdp = await connectPage(9222);
  await cdp.eval(`(async () => {
    window.__e2e = {
      app: await import('/src/stores/app-state.ts'),
      session: await import('/src/stores/session-store.ts'),
      taskStore: await import('/src/stores/task-store.ts'),
      settingsStore: await import('/src/stores/settings-store.ts'),
    };
  })()`);

  // Open the bench workspace and load the 10k document.
  console.log('[bench] opening workspace', DATASET_DIR);
  await ev(cdp, `await app.openWorkspace(${JSON.stringify(DATASET_DIR)}, 'bench-10k')`);
  await ev(cdp, `await app.scanAndIndex()`);
  const docs = await ev(cdp, `return app.documents.value.map(d => ({ id: d.id, path: d.file_path || d.path }))`);
  const doc = (docs || []).find((d) => (d.path || '').includes('scale-10k'));
  if (!doc) throw new Error('scale-10k document not registered: ' + JSON.stringify(docs));
  console.log('[bench] document:', doc.id);

  await ev(cdp, `
    await session.ensureSessionForDocument(${JSON.stringify(doc.id)});
    session.setActiveDocument(${JSON.stringify(doc.id)});
    await taskStore.loadTasks(${JSON.stringify(doc.id)});
  `);
  await waitPage(cdp, `document.querySelectorAll('.task-row').length > 0`, 60000, 'initial rows');

  // Expand everything so all 10k tasks are visible rows.
  const counts = await ev(cdp, `
    app.expandedTaskKeys.value = new Set(taskStore.allTasks.value.map(t => t.task_key));
    return { tasks: taskStore.allTasks.value.length, visible: taskStore.visibleRows.value.length };
  `);
  console.log('[bench] tasks:', counts.tasks, 'visible rows:', counts.visible);
  if (counts.visible < 10000) {
    console.warn('[bench] WARNING: visible rows below 10k gate threshold:', counts.visible);
  }

  const report = {
    scenario: 'ui-tree-render-10k',
    dataset: DATASET_DIR,
    tasks: counts.tasks,
    visible_rows: counts.visible,
    measured_at: new Date().toISOString(),
    runs: {},
  };

  for (const mode of ['off', 'on']) {
    console.log(`[bench] virtualization ${mode}: switching + settling`);
    await ev(cdp, `settingsStore.settings.virtualization.mode = '${mode}'; return true;`);
    // Wait for the DOM to reflect the mode: off → all rows mounted; on → small window.
    if (mode === 'off') {
      await waitPage(cdp,
        `document.querySelectorAll('.task-row').length >= ${Math.min(counts.visible, 10000)}`,
        180000, 'full 10k DOM mount (virtualization off)');
    } else {
      await waitPage(cdp,
        `document.querySelectorAll('.task-row').length < 200`,
        60000, 'virtual window mount (virtualization on)');
    }
    await sleep(1500); // settle after the big patch

    const mounted = await cdp.eval(`document.querySelectorAll('.task-row').length`);
    console.log(`[bench] mode=${mode} mounted rows=${mounted}; measuring 3s scroll`);
    const deltas = await ev(cdp, MEASURE_EXPR(3000));
    const stats = summarize(deltas);
    stats.mounted_dom_rows = mounted;
    report.runs[mode] = stats;
    console.log(`[bench] mode=${mode}`, JSON.stringify(stats));
    await shot(cdp, `m10-render-bench-${mode}`);
  }

  // Decision gate per PROFILING_METHOD.md §4.2 (RD-M10-022).
  const p95off = report.runs.off.p95_ms;
  report.decision_gate = {
    rule: 'RD-M10-022: p95 frame time > 16 ms at 10k+ visible rows → virtualization IMPLEMENT',
    p95_virtualization_off_ms: p95off,
    p95_virtualization_on_ms: report.runs.on.p95_ms,
    virtualization_required: p95off > 16,
    verdict: p95off > 16
      ? 'virtualization IMPLEMENT is mandatory at 10k (already shipped; auto mode must engage)'
      : 'plain list within frame budget at 10k; virtualization optional',
  };

  const out = path.join(EVID, 'm10-render-bench.json');
  fs.writeFileSync(out, JSON.stringify(report, null, 2));
  console.log('[bench] wrote', out);
  cdp.close();
  process.exit(0);
}

async function shot(cdp, name) {
  const p = path.join(EVID, `${name}.png`);
  await cdp.screenshot(p);
  console.log('  [shot]', p);
}

main().catch((e) => { console.error('BENCH FAIL:', e.message); process.exit(1); });
