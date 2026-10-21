# Leftover / Deferred Tasks

Goal instruction (2026-10-21): bypassed blockers and deferred items are recorded
here instead of stopping the plan. Each entry lists the bypass reason and the
intended resolution point.

## L-01: scale-100k benchmark dataset never completed

- **What**: `node scripts/bench/run-benchmark.cjs --scale all --release`
  produced reports for scale-1k/10k/50k only; scale-100k was killed mid-run
  (stale process cleanup) and never re-run to completion.
- **Why deferred**: `index_rebuild` is superlinear (13.7s@1k → 140s@10k →
  1266s@50k); scale-100k would take ~1h+ for the rebuild step alone and is not
  a 2.0 RC gate (no RD/QA item requires scale-100k numbers).
- **Resolution**: optional post-GA; if needed, run
  `MTL_BENCH_SCALE=all` overnight. The 50k numbers already document the
  scaling characteristic.

## L-02: Release-build render frame re-measurement

- **What**: RD-M10-022 measurement (`docs/qa/evidence/m10-render-bench.json`)
  ran on the debug build under CDP. Virtualization OFF p95=34.5ms (>16ms →
  virtualization required, gate satisfied), but virtualization ON p95=28ms is
  also above 16ms; the excess is attributed to debug+CDP overhead.
- **Why deferred**: the decision gate only asks whether virtualization is
  required (answered: yes, and it is shipped with auto mode engaging at
  threshold=200). Absolute frame budget on release builds is a clean-machine
  concern.
- **Resolution**: re-run `scripts/e2e/m10-render-bench.cjs` against the frozen
  RC build during QA-M10-H (INH-1131) on a clean VM.

## L-04: QA-M10-H01~H15 clean-machine VM matrix (INH-1131) — environment blocked

- **What**: The H-matrix requires clean Hyper-V VMs (Win10 22H2, Win11 24H2,
  no-WebView2 snapshot, zh-CN locale snapshot) per
  `docs/qa/QA_M10_H_PORTABLE_PROTOCOL.md`.
- **Why blocked**: host is Windows 11 Pro with the hypervisor present, but the
  Hyper-V feature/PowerShell module is NOT enabled (Get-VM unavailable), no
  VirtualBox/VMware is installed, and no Windows ISO images exist on disk.
  Enabling Hyper-V requires admin elevation plus a machine reboot, which would
  terminate the running dev/test session; creating VMs additionally requires
  multi-GB ISO downloads. This is an external environment dependency, not a
  code issue.
- **Bypass**: P4 skipped; P5/P6 proceed on host-based evidence. INH-1131
  remains In Progress with a Linear comment describing exactly what is needed.
- **Resolution**: user action — enable Hyper-V (`dism /online /enable-feature
  /featurename:Microsoft-Hyper-V /all`, reboot) or install VirtualBox, provide
  Win10/Win11 ISOs, then run P4a (pre-freeze dry run) and P4b (formal matrix
  on the frozen RC ZIP). Also covers L-02 (release-build render re-measure)
  once VMs exist.

## L-03: Target-parent selector for cross-document transfer

- **What**: M8 transfer UX inserts moved/copied subtrees at the target
  document root (`append_subtree_to_root` semantics); picking a specific
  parent task in the target document is not exposed in the UI.
- **Why deferred**: explicit scope constraint in the approved plan (GA 后增强);
  root-level insertion is lossless and undoable.
- **Resolution**: post-2.0 enhancement issue.
