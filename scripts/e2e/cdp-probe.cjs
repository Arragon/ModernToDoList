// Probe the running app's UI state over CDP. Usage: node cdp-probe.cjs <outPrefix>
const { connectPage } = require('./cdp-client.cjs');

(async () => {
  const outPrefix = process.argv[2] || 'docs/qa/evidence/e2e-probe';
  const cdp = await connectPage(9222);
  // wait for vite + app mount (up to 30s)
  let ready = false;
  for (let i = 0; i < 60; i++) {
    const ok = await cdp.eval(`!!document.querySelector('.app-root') && document.querySelectorAll('*').length > 50`).catch(() => false);
    if (ok) { ready = true; break; }
    await new Promise((r) => setTimeout(r, 500));
  }
  const state = await cdp.eval(`(() => {
    const txt = (sel) => Array.from(document.querySelectorAll(sel)).map(e => e.textContent.trim().slice(0, 80));
    return {
      title: document.title,
      url: location.href,
      ready: ${ready},
      bodyClasses: document.body.className,
      sidebarItems: txt('.sidebar__item'),
      sidebarHeaders: txt('.sidebar__section-title, .sidebar__header, .sidebar h2, .sidebar h3'),
      taskRows: txt('[class*="task-row"], [class*="taskrow"], [class*="TaskRow"]').slice(0, 20),
      taskRowCount: document.querySelectorAll('[class*="task-row"], [class*="taskrow"]').length,
      loading: txt('[class*="loading"]').slice(0, 5),
      buttons: txt('button').slice(0, 30),
      allClasses: Array.from(new Set(Array.from(document.querySelectorAll('[class]')).flatMap(e => String(e.className).split(/\\s+/)))).filter(c => c.length > 2).slice(0, 200),
    };
  })()`);
  console.log(JSON.stringify(state, null, 2));
  await cdp.screenshot(outPrefix + '.png');
  require('fs').writeFileSync(outPrefix + '.json', JSON.stringify(state, null, 2));
  cdp.close();
  process.exit(0);
})().catch((e) => { console.error('PROBE FAIL:', e.message); process.exit(1); });
