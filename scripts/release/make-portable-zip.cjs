#!/usr/bin/env node
/**
 * make-portable-zip.cjs — deterministic Portable release ZIP builder
 * (RD-M10-024 / INH-1117).
 *
 * Produces a *reproducible* archive: the same staging directory always
 * yields byte-identical output, so the published SHA-256 is stable across
 * rebuilds. Determinism is achieved by:
 *   1. Sorting entries by archive path (byte order, '/' separators).
 *   2. Stamping every entry with a fixed DOS timestamp (2025-01-01 00:00:00).
 *   3. Using one compression setting for all files (DEFLATE, level 9) and
 *      STORE for directories.
 *   4. Writing no extra fields (no UNIX timestamps, no UID/GID).
 *
 * This mirrors the canonical Rust implementation in
 * src-tauri/src/release/packaging.rs (which is covered by unit tests). This
 * script is the dependency-free build-machine entry point; it uses only Node
 * built-ins (zlib, crypto, fs).
 *
 * Usage:
 *   node scripts/release/make-portable-zip.cjs <stagingDir> <outDir> [version]
 *
 * Outputs (in outDir):
 *   ModernToDoList-<version>-Portable-win-x64.zip
 *   ModernToDoList-<version>-Portable-win-x64.zip.sha256
 */

'use strict';

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');

const FIXED_DOS_TIME = 0x0000; // 00:00:00
const FIXED_DOS_DATE = ((2025 - 1980) << 9) | (1 << 5) | 1; // 2025-01-01
const DEFLATE_LEVEL = 9;

// ---------------------------------------------------------------------------
// CRC-32 (IEEE, as required by the ZIP format)
// ---------------------------------------------------------------------------
const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[i] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

// ---------------------------------------------------------------------------
// Staging directory walk (deterministic order)
// ---------------------------------------------------------------------------
function walk(root) {
  const entries = [];
  const visit = (dir, prefix) => {
    const names = fs.readdirSync(dir).sort(); // sorted => deterministic
    for (const name of names) {
      const abs = path.join(dir, name);
      const rel = prefix ? `${prefix}/${name}` : name;
      const st = fs.statSync(abs); // no symlink following beyond stat
      if (st.isDirectory()) {
        entries.push({ path: `${rel}/`, isDir: true, contents: Buffer.alloc(0) });
        visit(abs, rel);
      } else if (st.isFile()) {
        entries.push({ path: rel, isDir: false, contents: fs.readFileSync(abs) });
      }
      // Symlinks/other types are skipped for reproducibility.
    }
  };
  visit(root, '');
  entries.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return entries;
}

// ---------------------------------------------------------------------------
// ZIP writer (local file headers + central directory, no data descriptors)
// ---------------------------------------------------------------------------
function buildZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;

  for (const e of entries) {
    const nameBuf = Buffer.from(e.path, 'utf8');
    const crc = crc32(e.contents);
    const uncompressedSize = e.contents.length;

    let compressed;
    let method;
    if (e.isDir) {
      compressed = Buffer.alloc(0);
      method = 0; // STORE
    } else {
      compressed = zlib.deflateRawSync(e.contents, { level: DEFLATE_LEVEL });
      method = 8; // DEFLATE
    }

    const flags = 0x0800; // UTF-8 file names (no data descriptor bit)
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4); // version needed
    local.writeUInt16LE(flags, 6);
    local.writeUInt16LE(method, 8);
    local.writeUInt16LE(FIXED_DOS_TIME, 10);
    local.writeUInt16LE(FIXED_DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(compressed.length, 18);
    local.writeUInt32LE(uncompressedSize, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28); // extra length

    locals.push(local, nameBuf, compressed);

    const externalAttrs = e.isDir ? 0x10 : 0; // DOS directory bit
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4); // version made by
    central.writeUInt16LE(20, 6); // version needed
    central.writeUInt16LE(flags, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt16LE(FIXED_DOS_TIME, 12);
    central.writeUInt16LE(FIXED_DOS_DATE, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(compressed.length, 20);
    central.writeUInt32LE(uncompressedSize, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt16LE(0, 30); // extra
    central.writeUInt16LE(0, 32); // comment
    central.writeUInt16LE(0, 34); // disk number
    central.writeUInt16LE(0, 36); // internal attrs
    central.writeUInt32LE(externalAttrs, 38);
    central.writeUInt32LE(offset, 42);

    centrals.push(central, nameBuf);
    offset += local.length + nameBuf.length + compressed.length;
  }

  const centralBuf = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBuf.length, 12);
  end.writeUInt32LE(offset, 16);
  end.writeUInt16LE(0, 20);

  return Buffer.concat([...locals, centralBuf, end]);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
function main(argv) {
  const [, , stagingDir, outDir, version = '2.0.0'] = argv;
  if (!stagingDir || !outDir) {
    console.error('Usage: node make-portable-zip.cjs <stagingDir> <outDir> [version]');
    process.exit(2);
  }
  if (!fs.statSync(stagingDir).isDirectory()) {
    console.error(`Staging directory not found: ${stagingDir}`);
    process.exit(2);
  }

  const zipName = `ModernToDoList-${version}-Portable-win-x64.zip`;
  fs.mkdirSync(outDir, { recursive: true });

  const entries = walk(stagingDir);
  const zip = buildZip(entries);
  const sha256 = crypto.createHash('sha256').update(zip).digest('hex');

  const zipPath = path.join(outDir, zipName);
  fs.writeFileSync(zipPath, zip);
  fs.writeFileSync(path.join(outDir, `${zipName}.sha256`), `${sha256}  ${zipName}\n`);

  console.log(`Wrote ${zipPath} (${zip.length} bytes, ${entries.length} entries)`);
  console.log(`SHA-256: ${sha256}`);

  // Self-check: rebuild and verify byte-identical output (determinism proof).
  const again = buildZip(walk(stagingDir));
  if (!again.equals(zip)) {
    console.error('FATAL: rebuild produced different bytes — determinism broken');
    process.exit(1);
  }
  console.log('Determinism check: rebuild produced identical bytes.');
}

main(process.argv);
