# RC-M10 Blocker Review and 2.0 Scope Freeze

Generated: 2026-10-21T06:40:15.276Z
Source: Linear project 72e576a7-e894-4e22-b52c-5d47d43a5912

## Verdict

**Open M6-M10 issues: 4. Blocker count: 4.**

GATE-M10 (INH-1133) requires **all Blockers = 0** and **all P0/P1 data-safety issues = 0**. That precondition is NOT satisfied, so the GA gate cannot be passed and must remain open.

## Summary

| Category | Open | P1/Urgent+High |
|----------|------|----------------|
| Blocker-Data | 0 | 0 |
| Blocker-Persistence | 0 | 0 |
| Blocker-Portable | 2 | 2 |
| Blocker-Security | 0 | 0 |
| Blocker-Core-UX | 2 | 2 |
| **Total** | **4** | **4** |

## By milestone

| Milestone | Open issues |
|-----------|-------------|
| M10 | 4 |

## Blocker-Data (0)

_None open._

## Blocker-Persistence (0)

_None open._

## Blocker-Portable (2)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-718 | Urgent | Backlog | M10 Epic: Release Candidate / GA |
| INH-1131 | Urgent | In Progress | QA-M10-H01~H15 Windows Portable and clean-machine RC matrix |

## Blocker-Security (0)

_None open._

## Blocker-Core-UX (2)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1132 | Urgent | In Progress | RC-M10 Blocker review and 2.0 scope freeze |
| INH-1133 | Urgent | Backlog | GATE-M10 ModernToDoList 2.0 GA Exit Gate |

## Gates that cannot be auto-verified in this environment

- **QA-M10-H (INH-1131)** requires execution on clean Windows 10 and Windows 11 VMs with no network, no administrator rights, and no Node/Rust toolchain installed. That is a manual, out-of-band activity and cannot be automated from a development workstation. Until it is executed by a human and the results recorded, GATE-M10 must not be closed.
- **RC-M10 (INH-1132)** additionally requires freezing the commit, the version string and the release ZIP SHA-256. Those can only be frozen once the blocker count above reaches 0.
