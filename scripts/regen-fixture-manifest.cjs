'use strict';

// Regenerate tests/fixtures/xml/MANIFEST.sha256 over every fixture on disk.
//
// The original manifest was hand-generated and covered only the 19 M0-M5
// fixtures. M6-M10 added relations/, richtext/, transfer/ and rc/ fixtures, so
// the manifest silently stopped describing the corpus it is supposed to
// guarantee. Format is preserved exactly: UTF-8 BOM, "# " header lines,
// uppercase SHA-256, two spaces, POSIX-relative path.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const ROOT = path.join(__dirname, '..', 'tests', 'fixtures', 'xml');
const MANIFEST = path.join(ROOT, 'MANIFEST.sha256');

function collect(dir, base = '') {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const rel = base ? `${base}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...collect(path.join(dir, entry.name), rel));
    else if (entry.name.endsWith('.xml')) out.push(rel);
  }
  return out.sort();
}

const files = collect(ROOT);
const now = new Date();
const pad = (n) => String(n).padStart(2, '0');
const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;

const lines = [
  '\uFEFF# SHA-256 Hash Manifest for XML Test Fixtures',
  `# Generated: ${stamp}`,
  '# Base Path: tests/fixtures/xml',
  `# Fixture Count: ${files.length}`,
  '',
];

for (const rel of files) {
  const hash = crypto.createHash('sha256').update(fs.readFileSync(path.join(ROOT, rel))).digest('hex').toUpperCase();
  lines.push(`${hash}  ${rel}`);
}

fs.writeFileSync(MANIFEST, lines.join('\n') + '\n', 'utf8');
console.log(`Wrote ${MANIFEST}`);
console.log(`  ${files.length} fixtures`);

// Self-verify: re-read and re-hash every listed entry.
const body = fs.readFileSync(MANIFEST, 'utf8').replace(/^\uFEFF/, '');
let verified = 0;
for (const line of body.split(/\r?\n/)) {
  if (!line || line.startsWith('#')) continue;
  const [expected, rel] = line.split(/\s{2,}/);
  const actual = crypto.createHash('sha256').update(fs.readFileSync(path.join(ROOT, rel))).digest('hex').toUpperCase();
  if (actual !== expected) {
    console.error(`MISMATCH: ${rel}\n  expected ${expected}\n  actual   ${actual}`);
    process.exit(1);
  }
  verified++;
}
console.log(`  self-verified ${verified}/${files.length} hashes`);
