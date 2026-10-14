'use strict';

// Idempotently declare every Rust module that exists on disk but is not yet
// listed in its parent mod file. Multiple agents add modules concurrently and
// are forbidden from editing the shared mod files, so the orchestrator runs this
// once to wire everything up.
//
// Usage: node scripts/wire-modules.cjs [--dry-run]

const fs = require('fs');
const path = require('path');

const SRC = path.join(__dirname, '..', 'src-tauri', 'src');
const DRY = process.argv.includes('--dry-run');

// parent mod file -> directory to scan for child modules
const TARGETS = [
  { mod: path.join(SRC, 'domain', 'mod.rs'), dir: path.join(SRC, 'domain') },
  { mod: path.join(SRC, 'infrastructure', 'mod.rs'), dir: path.join(SRC, 'infrastructure') },
  { mod: path.join(SRC, 'commands', 'mod.rs'), dir: path.join(SRC, 'commands') },
  { mod: path.join(SRC, 'platform', 'windows', 'mod.rs'), dir: path.join(SRC, 'platform', 'windows') },
];

// Top-level modules declared directly in lib.rs.
const LIB = path.join(SRC, 'lib.rs');
const LIB_DIRS = ['benchmark', 'release', 'diagnostics', 'domain', 'infrastructure', 'commands', 'platform', 'application'];

function childModules(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isFile() && entry.name.endsWith('.rs') && entry.name !== 'mod.rs') {
      out.push(entry.name.replace(/\.rs$/, ''));
    } else if (entry.isDirectory() && fs.existsSync(path.join(dir, entry.name, 'mod.rs'))) {
      out.push(entry.name);
    }
  }
  return out.sort();
}

function declaredNames(content) {
  const names = new Set();
  const re = /^\s*(?:pub\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/gm;
  let m;
  while ((m = re.exec(content))) names.add(m[1]);
  return names;
}

let added = 0;

function wire(modFile, dir, label) {
  if (!fs.existsSync(modFile)) {
    console.log(`SKIP  ${label}: ${path.relative(SRC, modFile)} does not exist`);
    return;
  }
  const original = fs.readFileSync(modFile, 'utf8');
  const declared = declaredNames(original);
  const missing = childModules(dir).filter((name) => !declared.has(name));
  if (!missing.length) {
    console.log(`OK    ${label}: all ${declared.size} modules declared`);
    return;
  }

  const eol = original.includes('\r\n') ? '\r\n' : '\n';
  const block = `${eol}// Wired by scripts/wire-modules.cjs${eol}` + missing.map((n) => `pub mod ${n};`).join(eol) + eol;

  // Insert the declarations right after the last existing `mod` line so the
  // re-export section below it stays intact.
  const lines = original.split(eol);
  let lastMod = -1;
  lines.forEach((line, i) => {
    if (/^\s*(?:pub\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;/.test(line)) lastMod = i;
  });

  let next;
  if (lastMod >= 0) {
    lines.splice(lastMod + 1, 0, '', ...missing.map((n) => `pub mod ${n};`));
    next = lines.join(eol);
  } else {
    next = original.replace(/\s*$/, '') + block;
  }

  if (!DRY) fs.writeFileSync(modFile, next, 'utf8');
  added += missing.length;
  console.log(`${DRY ? 'DRY ' : 'WIRE'}  ${label}: + ${missing.join(', ')}`);
}

for (const target of TARGETS) {
  wire(target.mod, target.dir, path.relative(SRC, target.dir));
}

// lib.rs top-level modules
if (fs.existsSync(LIB)) {
  const original = fs.readFileSync(LIB, 'utf8');
  const declared = declaredNames(original);
  const missing = LIB_DIRS.filter((name) => {
    const p = path.join(SRC, name);
    if (!fs.existsSync(p)) return false;
    const stat = fs.statSync(p);
    return (stat.isDirectory() && fs.existsSync(path.join(p, 'mod.rs'))) || !stat.isDirectory();
  }).filter((name) => !declared.has(name));

  if (!missing.length) {
    console.log(`OK    lib.rs: all top-level modules declared`);
  } else {
    const eol = original.includes('\r\n') ? '\r\n' : '\n';
    const lines = original.split(eol);
    let lastMod = -1;
    lines.forEach((line, i) => {
      if (/^\s*(?:pub\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;/.test(line)) lastMod = i;
    });
    lines.splice(lastMod + 1, 0, '', ...missing.map((n) => `pub mod ${n};`));
    if (!DRY) fs.writeFileSync(LIB, lines.join(eol), 'utf8');
    added += missing.length;
    console.log(`${DRY ? 'DRY ' : 'WIRE'}  lib.rs: + ${missing.join(', ')}`);
  }
}

console.log(`\n${DRY ? 'Would add' : 'Added'} ${added} module declaration(s).`);
