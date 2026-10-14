# QA-M10-H01~H15 — Windows Portable / Clean-Machine RC Matrix

**Linear:** INH-1131 · **Milestone:** M10 · **Priority:** P1 · **Gate:** GATE-M10 (INH-1133)

This matrix **cannot be automated from a development workstation**. It verifies that the
Portable build runs on a machine that has never seen the toolchain, so every case below must be
executed by a human inside the specified VM. This document is the executable protocol: each case
states its preconditions, steps, the exact expected result, and the evidence to capture.

Record results in the table at the bottom, then attach it to INH-1131 as a comment via
`node scripts/linear-sync.cjs scripts/comments/<batch>.json` so the body is UTF-8 encoded.

---

## Environment preconditions (apply to every case unless overridden)

| Requirement | Value |
|---|---|
| Guest OS | Windows 10 22H2 x64 **and** Windows 11 24H2 x64 — run the whole matrix on both |
| VM snapshot | Clean OS install, no dev tooling, take a snapshot before each destructive case |
| Network | **Disconnected** (disable the vNIC). Only H14 may enable it |
| Account | Standard user, **not** an administrator, UAC at default level |
| Installed runtimes | No Node.js, no Rust/cargo, no Python, no Git, no Visual Studio, no MSVC build tools |
| WebView2 | Must be absent for H02; present for all other cases |
| Input | The release ZIP `ModernToDoList-2.0.0-Portable-win-x64.zip` plus its published SHA-256 |
| Locale | System locale zh-CN with Chinese display language for H08–H10 |

**Why these constraints:** a Portable app that only works on the developer's machine is not
portable. The clean-VM, no-admin, no-network, no-toolchain combination is what distinguishes a
genuinely self-contained release from one that silently leans on the build environment.

---

## Cases

### H01 — ZIP integrity and hash match
Verify the downloaded ZIP's SHA-256 equals the published hash (`certutil -hashfile <zip> SHA256`).
**Pass:** hashes match exactly. **Fail:** any difference — stop the matrix, the artifact is not the frozen RC.

### H02 — WebView2 absent produces an actionable error
On a VM with no WebView2 Runtime, launch the EXE.
**Pass:** the app does not vanish silently; it shows a structured, localized message naming the
missing component and how to obtain it, matching `docs/release/WEBVIEW2_POLICY.md`, and exits with
a non-zero code. A crash dump or blank window is a **fail**.

### H03 — First launch with no network
Network disconnected, WebView2 present. Launch from `C:\Portable\ModernToDoList\`.
**Pass:** the app reaches its empty workspace state with no network prompt, no telemetry attempt
(confirm zero outbound connections in the VM's network log), and no hang longer than 5 s.

### H04 — No administrator rights
As a standard user, launch, create a workspace, add and complete a task, close.
**Pass:** every operation succeeds with no UAC prompt. **Fail:** any elevation request.

### H05 — Data directory is writable next to a read-only EXE
Place the app in a directory whose ACL denies write to the standard user (read-only EXE dir).
**Pass:** per RD-M10-033 the app relocates `Data/` to a writable location under the user profile,
tells the user where, and all functions work. **Fail:** a write error, a crash, or silent data loss.

### H06 — Move the whole application directory
With an existing workspace containing ≥20 tasks, move the app folder to a different drive, then relaunch.
**Pass:** the workspace is still found or can be re-opened; task data, index and settings survive;
no stale absolute path is dereferenced. Capture the before/after task count — they must be equal.

### H07 — Removable drive and drive-letter change
Run the app from a USB stick. Create a workspace, then reinsert the stick so Windows assigns a
different letter, and relaunch.
**Pass:** per RD-M10-030 the app detects the letter change and recovers via a stored relative path or
volume identity, reporting clearly if the volume is genuinely absent. **Fail:** a corrupt index, a
crash, or tasks silently disappearing.

### H08 — Chinese characters in every path
Install to `D:\我的项目\现代待办清单 2.0\`, workspace at `D:\数据\工作清单\我的任务.xml`, with a task
titled `买牛奶 @张三 #生活 !高 due:2026-12-31`.
**Pass:** all paths display correctly (no mojibake, no `????`), the file saves and reopens byte-stable,
and search finds the Chinese title. Verify the on-disk XML still declares its original encoding.

### H09 — Spaces and special characters in paths
Install to `C:\Program Files (x86)\Modern To-Do List [2.0] (Portable)\` and use a workspace named
`my task's list & notes.xml`.
**Pass:** no quoting or escaping bug; save, reopen, index rebuild and attachment import all work.

### H10 — Long paths beyond 260 characters
Create a workspace whose full path exceeds 260 characters via deeply nested Chinese and ASCII folders,
then add an attachment so the managed asset path exceeds it further.
**Pass:** per RD-M10-032 the `\\?\` prefix is applied and every operation succeeds. **Fail:** any
"path too long" error, truncation, or a partially written file.

### H11 — Index rebuild after deleting index.db
Close the app, delete `index.db` from the Data directory, relaunch.
**Pass:** the index rebuilds from the XML scan and **every** function works — search, smart views,
saved views, filters, task counts. This is an explicit GATE-M10 verification item. Compare search
results before and after; they must be identical.

### H12 — Atomic save survives a hard kill mid-write
With a ≥1000-task document, trigger a save and hard-kill the VM (or the process) during the write.
Repeat at least 5 times at varied moments.
**Pass:** per QA-M10-B the original document is never left corrupted or truncated — recovery yields
the last good version, or a duplicate rather than a loss. **Fail:** any zero-byte, truncated or
unparseable XML. Record the surviving file's SHA-256 for each attempt.

### H13 — Crash diagnostics are privacy-safe
Force a crash (see the app's diagnostics export action), then open the exported bundle.
**Pass:** per RD-M10-036 the bundle contains a timestamp, stack trace, last operations and system
info, but **no task content and no verbatim file paths** — paths appear only as hashes. Grep the whole
bundle for a known task title and for the Chinese folder name used in H08; both must be absent.

### H14 — Antivirus interaction
With network enabled and Windows Defender active at default settings, run a full scan on the app
directory, then launch.
**Pass:** no quarantine and no block. If Defender flags it, record the exact detection name and follow
`docs/release/ANTIVIRUS_FALSE_POSITIVES.md`; a detection is a **Blocker-Portable** for RC-M10.

### H15 — Manual update path preserves user data
With a populated workspace, replace the app binaries by following `docs/release/MANUAL_UPDATE.md`
using a newer ZIP, keeping the existing `Data/` directory.
**Pass:** the documented steps alone are sufficient (no undocumented action needed), the version
metadata updates, and all tasks, settings, saved views and index survive. Migration per RD-M10-025~027
runs automatically and is reported.

---

## Result record

Fill one row per case per OS. Evidence = screenshot path, log excerpt, or file hash.

| Case | Win10 | Win11 | Evidence | Notes / defect id |
|------|-------|-------|----------|-------------------|
| H01 | | | | |
| H02 | | | | |
| H03 | | | | |
| H04 | | | | |
| H05 | | | | |
| H06 | | | | |
| H07 | | | | |
| H08 | | | | |
| H09 | | | | |
| H10 | | | | |
| H11 | | | | |
| H12 | | | | |
| H13 | | | | |
| H14 | | | | |
| H15 | | | | |

Mark each cell `PASS`, `FAIL` or `BLOCKED`. Any `FAIL` on H05, H07, H10, H11, H12 or H13 is a
data-safety or portability blocker and **blocks GATE-M10 outright** — those cases protect against
losing a user's tasks, which is the one failure the product cannot recover from.

## Status

**Not executed.** As of 2026-09-22 no case in this matrix has been run: the required clean Windows
VMs are not available in this environment. INH-1131 therefore remains open and is counted as a
blocker in `docs/release/RC_M10_BLOCKER_REVIEW.md`. Do not close GATE-M10 on the strength of
automated tests alone — this matrix is an explicit gate requirement.
