// Reload the app webview page via CDP. Usage: node cdp-reload.cjs
const { connectPage } = require('./cdp-client.cjs');
(async () => {
  const cdp = await connectPage(9222);
  await cdp.send('Page.enable');
  await cdp.send('Page.reload', { ignoreCache: true });
  await new Promise((r) => setTimeout(r, 4000));
  console.log('url=', await cdp.eval('location.href'));
  cdp.close();
  process.exit(0);
})().catch((e) => { console.error('FAIL', e.message); process.exit(1); });
