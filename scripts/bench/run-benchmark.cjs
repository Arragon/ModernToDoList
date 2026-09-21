#!/usr/bin/env node
'use strict';
/**
 * ModernToDoList benchmark runner (M10 / INH-1114).
 *
 * Invokes the compiled Rust benchmark harness (the ignored integration test
 * `run_full_benchmark` in src-tauri/tests/m10_benchmark_tests.rs) via cargo,
 * which:
 *   1. generates the deterministic fixture datasets (scale/shape variants
 *      selected by --scale, SHA-256 manifest included),
 *   2. profiles the real code paths (startup, workspace scan, XML parse,
 *      index rebuild, search latency, process memory),
 *   3. writes versioned JSON + Markdown reports.
 *
 * This script is UTF-8 safe: all child output is decoded as UTF-8, all files
 * are read/written with an explicit 'utf8' encoding, and no shell
 * interpolation of report content ever happens.
 *
 * Usage:
 *   node scripts/bench/run-benchmark.cjs [--scale small|shapes|medium|large|all]
 *                                        [--out <dir>] [--seed <u64>]
 *                                        [--release] [--cargo <path>]
 *
 * Exit codes: 0 = success, 1 = harness failure, 2 = bad usage, 3 = bad reports.
 */

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const SRC_TAURI = path.join(REPO_ROOT, 'src-tauri');

function parseArgs(argv) {
  const opts = {
    scale: process.env.MTL_BENCH_SCALE || 'small',
    out: null,
    seed: process.env.MTL_BENCH_SEED || null,
    release: false,
    cargo: process.env.CARGO || 'cargo',
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) {
        console.error(`ERROR: missing value for ${a}`);
        process.exit(2);
      }
      return argv[i];
    };
    switch (a) {
      case '--scale': opts.scale = next(); break;
      case '--out': opts.out = next(); break;
      case '--seed': opts.seed = next(); break;
      case '--release': opts.release = true; break;
      case '--cargo': opts.cargo = next(); break;
      case '-h': case '--help':
        console.log(fs.readFileSync(__filename, 'utf8').split('*/')[0]);
        process.exit(0);
        break;
      default:
        console.error(`ERROR: unknown argument ${a}`);
        process.exit(2);
    }
  }
  const valid = ['small', 'shapes', 'medium', 'large', 'all'];
  if (!valid.includes(opts.scale)) {
    console.error(`ERROR: --scale must be one of ${valid.join(', ')}`);
    process.exit(2);
  }
  if (opts.seed !== null && !/^\d+$/.test(String(opts.seed))) {
    console.error('ERROR: --seed must be a non-negative integer (u64)');
    process.exit(2);
  }
  if (!opts.out) {
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    opts.out = path.join(REPO_ROOT, 'Data', 'bench', stamp);
  }
  opts.out = path.resolve(opts.out);
  return opts;
}

function runCargo(opts) {
  const args = ['test', '--package', 'moderntodolist', '--test', 'm10_benchmark_tests'];
  if (opts.release) args.push('--release');
  args.push('--', '--ignored', '--exact', 'run_full_benchmark', '--nocapture');

  const env = Object.assign({}, process.env, {
    MTL_BENCH_SCALE: opts.scale,
    MTL_BENCH_OUT: opts.out,
    // The dead system proxy (127.0.0.1:50095) breaks cargo network access.
    no_proxy: '*',
    NO_PROXY: '*',
    RUST_LOG: process.env.RUST_LOG || 'warn',
  });
  if (opts.seed !== null) env.MTL_BENCH_SEED = String(opts.seed);

  console.log(`[bench] cargo ${args.join(' ')} (cwd=${SRC_TAURI})`);
  console.log(`[bench] scale=${opts.scale} out=${opts.out} seed=${opts.seed || 'canonical'}`);

  return new Promise((resolve, reject) => {
    const child = spawn(opts.cargo, args, { cwd: SRC_TAURI, env });
    const stdoutChunks = [];
    const stderrChunks = [];
    child.stdout.on('data', (buf) => {
      stdoutChunks.push(buf);
      process.stdout.write(buf);
    });
    child.stderr.on('data', (buf) => {
      stderrChunks.push(buf);
      process.stderr.write(buf);
    });
    child.on('error', reject);
    child.on('close', (code) => {
      resolve({
        code,
        stdout: Buffer.concat(stdoutChunks).toString('utf8'),
        stderr: Buffer.concat(stderrChunks).toString('utf8'),
      });
    });
  });
}

function collectReportPaths(stdout) {
  const json = [];
  const md = [];
  for (const line of stdout.split(/\r?\n/)) {
    if (line.startsWith('BENCH_REPORT_JSON=')) json.push(line.slice('BENCH_REPORT_JSON='.length).trim());
    else if (line.startsWith('BENCH_REPORT_MD=')) md.push(line.slice('BENCH_REPORT_MD='.length).trim());
  }
  return { json, md };
}

function validateReports(paths) {
  const problems = [];
  const summaries = [];
  for (const p of paths.json) {
    let report;
    try {
      report = JSON.parse(fs.readFileSync(p, 'utf8'));
    } catch (e) {
      problems.push(`${p}: unreadable/invalid JSON (${e.message})`);
      continue;
    }
    if (report.version !== 1) problems.push(`${p}: unexpected schema version ${report.version}`);
    if (typeof report.timestamp !== 'string') problems.push(`${p}: missing timestamp`);
    if (typeof report.dataset !== 'string') problems.push(`${p}: missing dataset`);
    if (!Array.isArray(report.metrics)) problems.push(`${p}: missing metrics array`);
    else {
      for (const m of report.metrics) {
        if (typeof m.name !== 'string' || typeof m.value !== 'number' || typeof m.unit !== 'string') {
          problems.push(`${p}: malformed metric ${JSON.stringify(m)}`);
          break;
        }
        if (!('percentile' in m)) {
          problems.push(`${p}: metric ${m.name} missing percentile key`);
          break;
        }
      }
      summaries.push({
        file: path.basename(p),
        dataset: report.dataset,
        metrics: report.metrics.length,
        size: fs.statSync(p).size,
      });
    }
  }
  for (const p of paths.md) {
    try {
      const text = fs.readFileSync(p, 'utf8');
      if (!text.includes('Benchmark Report')) problems.push(`${p}: not a benchmark markdown report`);
    } catch (e) {
      problems.push(`${p}: unreadable (${e.message})`);
    }
  }
  return { problems, summaries };
}

async function main() {
  const opts = parseArgs(process.argv);
  fs.mkdirSync(opts.out, { recursive: true });

  let result;
  try {
    result = await runCargo(opts);
  } catch (e) {
    console.error(`[bench] failed to launch cargo: ${e.message}`);
    process.exit(1);
  }
  if (result.code !== 0) {
    console.error(`[bench] cargo exited with code ${result.code}`);
    process.exit(1);
  }
  if (!result.stdout.includes('BENCH_DONE')) {
    console.error('[bench] harness did not report completion (BENCH_DONE missing)');
    process.exit(1);
  }

  const paths = collectReportPaths(result.stdout);
  if (paths.json.length === 0) {
    console.error('[bench] no BENCH_REPORT_JSON lines found; nothing was profiled');
    process.exit(3);
  }
  const { problems, summaries } = validateReports(paths);
  if (problems.length > 0) {
    for (const p of problems) console.error(`[bench] INVALID REPORT ${p}`);
    process.exit(3);
  }

  console.log('');
  console.log('[bench] Reports:');
  for (const s of summaries) {
    console.log(`  - ${s.dataset}: ${s.metrics} metrics -> ${path.join(opts.out, 'reports', s.file)}`);
  }
  console.log(`[bench] Markdown reports: ${paths.md.length}`);
  console.log(`[bench] Output directory: ${opts.out}`);
  console.log('[bench] OK');
}

main().catch((e) => {
  console.error(`[bench] unexpected error: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
});
