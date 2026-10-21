// Ad-hoc CDP eval: node cdp-eval.cjs "<expression>"  (window.__e2e stores available)
const { connectPage } = require('./cdp-client.cjs');
(async () => {
  const expr = process.argv.slice(2).join(' ');
  if (!expr) { console.error('usage: node cdp-eval.cjs "<expression>"'); process.exit(2); }
  const cdp = await connectPage(9222);
  if (!(await cdp.eval('!!window.__e2e').catch(() => false))) {
    await cdp.eval(`(async () => {
      window.__e2e = {
        app: await import('/src/stores/app-state.ts'),
        session: await import('/src/stores/session-store.ts'),
        taskStore: await import('/src/stores/task-store.ts'),
        transfer: await import('/src/stores/transfer-store.ts'),
      };
    })()`);
  }
  const r = await cdp.eval(`(async () => { const { app, session, taskStore, transfer } = window.__e2e; ${expr} })()`);
  console.log(JSON.stringify(r, null, 2));
  cdp.close();
  process.exit(0);
})().catch((e) => { console.error('EVAL FAIL:', e.message); process.exit(1); });
