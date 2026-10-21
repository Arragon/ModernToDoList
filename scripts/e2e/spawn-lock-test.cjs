const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const p = path.join(os.tmpdir(), 'spawn-lock-test2.txt');
fs.writeFileSync(p, 'orig');
console.log('target:', p);
const ps = spawn('powershell.exe', ['-NoProfile', '-Command',
  `$f=[System.IO.File]::Open('${p}','Open','ReadWrite','Read'); Write-Output "LOCKED"; Start-Sleep 8; $f.Close(); Write-Output "RELEASED"`],
  { stdio: ['ignore', 'pipe', 'pipe'] });
let out = '';
ps.stdout.on('data', (d) => { out += d; process.stdout.write('[ps] ' + d); });
ps.stderr.on('data', (d) => { out += 'STDERR:' + d; process.stdout.write('[ps-err] ' + d); });
ps.on('error', (e) => console.log('SPAWN ERROR:', e.message));
const iv = setInterval(() => {
  if (out.includes('LOCKED')) {
    clearInterval(iv);
    fs.writeFileSync(p + '.tmp', 'new');
    try { fs.renameSync(p + '.tmp', p); console.log('RENAME SUCCEEDED (lock NOT working)'); }
    catch (e) { console.log('RENAME BLOCKED:', e.code); }
    process.exit(0);
  }
}, 200);
setTimeout(() => { console.log('TIMEOUT waiting for LOCKED, out=', JSON.stringify(out)); process.exit(1); }, 10000);
