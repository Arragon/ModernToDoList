# RC-M10 Blocker Review and 2.0 Scope Freeze

Generated: 2026-09-22T00:17:45.074Z
Source: Linear project 72e576a7-e894-4e22-b52c-5d47d43a5912

## Verdict

**Open M6-M10 issues: 27. Blocker count: 27.**

GATE-M10 (INH-1133) requires **all Blockers = 0** and **all P0/P1 data-safety issues = 0**. That precondition is NOT satisfied, so the GA gate cannot be passed and must remain open.

## Summary

| Category | Open | P1/Urgent+High |
|----------|------|----------------|
| Blocker-Data | 5 | 5 |
| Blocker-Persistence | 2 | 2 |
| Blocker-Portable | 3 | 3 |
| Blocker-Security | 0 | 0 |
| Blocker-Core-UX | 17 | 17 |
| **Total** | **27** | **27** |

## By milestone

| Milestone | Open issues |
|-----------|-------------|
| M10 | 7 |
| M6 | 7 |
| M7 | 4 |
| M8 | 3 |
| M9 | 6 |

## Blocker-Data (5)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1043 | High | In Progress | RD-M6-004~006 Implement Participant Inspector chips, add/remove and bulk edit |
| INH-1053 | High | In Progress | RD-M6-041~045 Implement Attachment list UI, drag-in, open/reveal and missing-file states |
| INH-1083 | Urgent | In Progress | RD-M8-024~031 Implement cross-document transfer UX: Copy/Move pickers, drag-to-document, progress and recovery results |
| INH-1128 | Urgent | Backlog | QA-M10-E01~E12 Relations, attachments and Rich Content RC matrix |
| INH-1129 | Urgent | Backlog | QA-M10-F01~F10 Cross-document transaction and crash recovery RC matrix |

## Blocker-Persistence (2)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1049 | High | In Progress | RD-M6-027~030 Implement Progress Link Inspector UI, open action, multiple links and unknown-extension preservation |
| INH-1101 | High | In Progress | RD-M9-028~030 Persist, rename/delete and render Saved Views |

## Blocker-Portable (3)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-718 | Urgent | Backlog | M10 Epic: Release Candidate / GA |
| INH-1103 | High | In Progress | RD-M9-035~041 Implement offline Quick Add grammar and safe fallback |
| INH-1131 | Urgent | In Progress | QA-M10-H01~H15 Windows Portable and clean-machine RC matrix |

## Blocker-Security (0)

_None open._

## Blocker-Core-UX (17)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-714 | High | Backlog | M6 Epic: Task Relations Beta |
| INH-715 | High | Backlog | M7 Epic: Rich Content Beta |
| INH-716 | Urgent | Backlog | M8 Epic: Multi-Document Beta |
| INH-717 | High | Backlog | M9 Epic: Productivity Feature Complete |
| INH-1044 | High | In Progress | RD-M6-007~008 Add Participant filter and Participant grouping view |
| INH-1047 | High | In Progress | RD-M6-017~022 Implement dependency picker, Inspector relation UI and degraded-reference states |
| INH-1060 | Urgent | Backlog | GATE-M6 Task Relations Beta Exit Gate |
| INH-1061 | Urgent | In Progress | RD-M7-001~002 Integrate minimal Tiptap OSS editor with explicit edit/preview modes |
| INH-1062 | High | In Progress | RD-M7-003~010 Implement approved rich-text formatting toolbar |
| INH-1074 | Urgent | Backlog | GATE-M7 Rich Content Beta Exit Gate |
| INH-1094 | Urgent | Backlog | GATE-M8 Multi-Document Beta Exit Gate |
| INH-1097 | Urgent | In Progress | RD-M9-008~013 Implement Global Search DTO/UI, keyboard navigation, task jump, Chinese fallback and incremental updates |
| INH-1102 | High | In Progress | RD-M9-031~034 Finalize Command Registry and Ctrl+K palette |
| INH-1109 | Urgent | Backlog | GATE-M9 Productivity Feature Complete Exit Gate |
| INH-1130 | Urgent | Backlog | QA-M10-G01~G10 Productivity search, views and Quick Add RC matrix |
| INH-1132 | Urgent | In Progress | RC-M10 Blocker review and 2.0 scope freeze |
| INH-1133 | Urgent | Backlog | GATE-M10 ModernToDoList 2.0 GA Exit Gate |

## Gates that cannot be auto-verified in this environment

- **QA-M10-H (INH-1131)** requires execution on clean Windows 10 and Windows 11 VMs with no network, no administrator rights, and no Node/Rust toolchain installed. That is a manual, out-of-band activity and cannot be automated from a development workstation. Until it is executed by a human and the results recorded, GATE-M10 must not be closed.
- **RC-M10 (INH-1132)** additionally requires freezing the commit, the version string and the release ZIP SHA-256. Those can only be frozen once the blocker count above reaches 0.
