# RC-M10 Blocker Review and 2.0 Scope Freeze

Generated: 2026-09-21T21:51:47.125Z
Source: Linear project 72e576a7-e894-4e22-b52c-5d47d43a5912

## Verdict

**Open M6-M10 issues: 94. Blocker count: 94.**

GATE-M10 (INH-1133) requires **all Blockers = 0** and **all P0/P1 data-safety issues = 0**. That precondition is NOT satisfied, so the GA gate cannot be passed and must remain open.

## Summary

| Category | Open | P1/Urgent+High |
|----------|------|----------------|
| Blocker-Data | 32 | 32 |
| Blocker-Persistence | 19 | 19 |
| Blocker-Portable | 7 | 7 |
| Blocker-Security | 6 | 6 |
| Blocker-Core-UX | 30 | 30 |
| **Total** | **94** | **94** |

## By milestone

| Milestone | Open issues |
|-----------|-------------|
| M10 | 25 |
| M6 | 20 |
| M7 | 15 |
| M8 | 18 |
| M9 | 16 |

## Blocker-Data (32)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1043 | High | Backlog | RD-M6-004~006 Implement Participant Inspector chips, add/remove and bulk edit |
| INH-1050 | Urgent | Backlog | RD-M6-031~032,036~038 Define Attachment domain and Managed Asset path policy |
| INH-1051 | Urgent | Backlog | RD-M6-033 Implement transactional Managed Attachment import |
| INH-1052 | Urgent | Backlog | RD-M6-034~035,039~040 Implement Linked/URL Attachment persistence and rebuildable attachment index |
| INH-1053 | High | Backlog | RD-M6-041~045 Implement Attachment list UI, drag-in, open/reveal and missing-file states |
| INH-1054 | Urgent | Backlog | RD-M6-046~048 Implement attachment removal semantics, hash metadata and orphan tracking |
| INH-1058 | Urgent | Backlog | QA-M6-011~017 Attachment lifecycle, relocation and integrity matrix |
| INH-1059 | Urgent | Backlog | QA-M6-018 Run M0–M5 cumulative regression with relation/attachment features enabled |
| INH-1065 | Urgent | Backlog | RD-M7-019~023 Implement clipboard/drag image import as Managed Assets with relative rich-text references |
| INH-1066 | Urgent | Backlog | RD-M7-024~025 Implement image orphan tracking and explicit safe asset GC |
| INH-1071 | Urgent | Backlog | QA-M7-010~015 Managed image asset lifecycle matrix |
| INH-1077 | Urgent | Backlog | RD-M8-001~005 Build TransferManifest, source subtree snapshot, target ID map and logical clone |
| INH-1079 | Urgent | Backlog | RD-M8-006~010 Rewrite transfer references and build asset transfer plan |
| INH-1080 | Urgent | Backlog | RD-M8-011~014 Stage target assets/XML, validate and commit Target first |
| INH-1081 | Urgent | Backlog | RD-M8-015~019 Commit Source deletion only after Target, journal phases and recover interrupted/duplicate states |
| INH-1082 | Urgent | Backlog | RD-M8-020~023 Implement transaction-level Undo, Copy/Move services and structured transfer errors |
| INH-1083 | Urgent | Backlog | RD-M8-024~031 Implement cross-document transfer UX: Copy/Move pickers, drag-to-document, progress and recovery results |
| INH-1085 | Urgent | Backlog | RD-M8-033~035 Implement Managed Document rename, move and duplicate operations |
| INH-1086 | Urgent | Backlog | RD-M8-036~038 Implement document Import as Managed, Link External and Remove Reference flows |
| INH-1087 | Urgent | Backlog | RD-M8-039~042 Implement Managed Document Trash manifest, restore and explicit permanent deletion |
| INH-1089 | Urgent | Backlog | QA-M8-001~006 Transfer first-half crash matrix: before target commit through source-delete entry |
| INH-1090 | Urgent | Backlog | QA-M8-007~012 Source-commit, journal-corruption and I/O failure matrix |
| INH-1091 | Urgent | Backlog | QA-M8-013~016 Dependency and attachment remap matrix |
| INH-1092 | Urgent | Backlog | QA-M8-017~019 File Library Trash/Linked/rename-reference regression matrix |
| INH-1093 | Urgent | Backlog | QA-M8-020 Run M0–M7 full cumulative regression with transfer/file-library features enabled |
| INH-1096 | High | Backlog | RD-M9-004~007 Index tags, participants, attachment names and Progress Link labels for search |
| INH-1104 | High | Backlog | RD-M9-042~043 Add optional global Quick Add shortcut with conflict handling |
| INH-1107 | High | Backlog | QA-M9-011~015 Quick Add, command conflict, jump and canonical-filter regression matrix |
| INH-1114 | High | Backlog | RD-M10-016~021 Profile tree render, edit/save, transfer, memory and WebView2 growth |
| INH-1125 | Urgent | Backlog | QA-M10-B01~B10 Atomic Save, Recovery and external-conflict RC matrix |
| INH-1128 | Urgent | Backlog | QA-M10-E01~E12 Relations, attachments and Rich Content RC matrix |
| INH-1129 | Urgent | Backlog | QA-M10-F01~F10 Cross-document transaction and crash recovery RC matrix |

## Blocker-Persistence (19)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1042 | Urgent | Backlog | RD-M6-001~003 Implement Participant domain, native XML mapping and suggestion index |
| INH-1046 | Urgent | Backlog | RD-M6-013~016 Implement dependency mutations, reverse index and cycle prevention |
| INH-1048 | Urgent | Backlog | RD-M6-023~026 Implement Progress Link domain, XML storage, URL validation and provider detection |
| INH-1049 | High | Backlog | RD-M6-027~030 Implement Progress Link Inspector UI, open action, multiple links and unknown-extension preservation |
| INH-1055 | Urgent | Backlog | QA-M6-001~002 Participant round-trip and unknown-data preservation matrix |
| INH-1056 | Urgent | Backlog | QA-M6-003~008 Dependency compatibility, graph-safety and index-rebuild matrix |
| INH-1064 | Urgent | Backlog | RD-M7-014~018 Implement Plain/HTML editable modes, explicit conversion and RTF/Unknown read-only protection |
| INH-1068 | High | Backlog | RD-M7-028 Extract safe plain text from task descriptions for search/indexing |
| INH-1069 | Urgent | Backlog | QA-M7-001~006 Comments type-preservation and conversion matrix |
| INH-1072 | Urgent | Backlog | QA-M7-016 Large rich-text editing does not snapshot/serialize whole document per keystroke |
| INH-1084 | High | Backlog | RD-M8-032 Create new Managed Document directly inside Workspace without Save As |
| INH-1095 | Urgent | Backlog | RD-M9-001~003 Add FTS5 task search schema for title and description text |
| INH-1100 | High | Backlog | RD-M9-022~027 Define structured Saved View predicate model and evaluator |
| INH-1101 | High | Backlog | RD-M9-028~030 Persist, rename/delete and render Saved Views |
| INH-1106 | High | Backlog | QA-M9-006~010 Smart/Saved View date boundaries and non-mutating persistence matrix |
| INH-1113 | High | Backlog | RD-M10-011~015 Profile startup, scan, XML parse, index rebuild and search |
| INH-1118 | Urgent | Backlog | RD-M10-025~027 Implement versioned Data/settings/index migration and rebuild fallback |
| INH-1124 | Urgent | Backlog | QA-M10-A01~A13 XML compatibility full RC matrix |
| INH-1126 | Urgent | Backlog | QA-M10-C01~C10 Workspace, SQLite rebuild and watcher RC matrix |

## Blocker-Portable (7)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-718 | Urgent | Backlog | M10 Epic: Release Candidate / GA |
| INH-1088 | Urgent | Backlog | RD-M8-043~045 Implement Reveal Document, document-operation Undo policy and path-reference repair |
| INH-1103 | High | Backlog | RD-M9-035~041 Implement offline Quick Add grammar and safe fallback |
| INH-1117 | High | Backlog | RD-M10-024 Produce deterministic versioned Portable release ZIP |
| INH-1121 | High | Backlog | RD-M10-030~033 Harden removable, Chinese/long paths and read-only program directories |
| INH-1123 | High | Backlog | RD-M10-037~040 Finalize licenses, version metadata, manual Portable update path and release notes |
| INH-1131 | Urgent | Backlog | QA-M10-H01~H15 Windows Portable and clean-machine RC matrix |

## Blocker-Security (6)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-1057 | Urgent | Backlog | QA-M6-009~010 Progress Link persistence and URL-security matrix |
| INH-1063 | Urgent | Backlog | RD-M7-011~013 Implement HTML whitelist sanitizer, paste sanitation and dangerous-URL filtering |
| INH-1070 | Urgent | Backlog | QA-M7-007~009 Rich-text XSS and dangerous URL sanitation matrix |
| INH-1119 | High | Backlog | RD-M10-028~029 Finalize WebView2 diagnostics and offline package policy |
| INH-1120 | High | Backlog | RD-M10-034 Establish antivirus false-positive investigation process |
| INH-1122 | High | Backlog | RD-M10-035~036 Export crash logs and privacy-safe diagnostics |

## Blocker-Core-UX (30)

| Issue | Pri | State | Title |
|-------|-----|-------|-------|
| INH-714 | High | Backlog | M6 Epic: Task Relations Beta |
| INH-715 | High | Backlog | M7 Epic: Rich Content Beta |
| INH-716 | Urgent | Backlog | M8 Epic: Multi-Document Beta |
| INH-717 | High | Backlog | M9 Epic: Productivity Feature Complete |
| INH-1044 | High | Backlog | RD-M6-007~008 Add Participant filter and Participant grouping view |
| INH-1045 | Urgent | Backlog | RD-M6-009~012 Implement `TaskRef` dependency domain and local/external/unresolved parsing |
| INH-1047 | High | Backlog | RD-M6-017~022 Implement dependency picker, Inspector relation UI and degraded-reference states |
| INH-1060 | Urgent | Backlog | GATE-M6 Task Relations Beta Exit Gate |
| INH-1061 | Urgent | Backlog | RD-M7-001~002 Integrate minimal Tiptap OSS editor with explicit edit/preview modes |
| INH-1062 | High | Backlog | RD-M7-003~010 Implement approved rich-text formatting toolbar |
| INH-1067 | Urgent | Backlog | RD-M7-026~027 Integrate Rich Text Undo semantics and autosave debounce |
| INH-1073 | Urgent | Backlog | QA-M7-017 Run M0–M6 cumulative regression with Rich Content enabled |
| INH-1074 | Urgent | Backlog | GATE-M7 Rich Content Beta Exit Gate |
| INH-1094 | Urgent | Backlog | GATE-M8 Multi-Document Beta Exit Gate |
| INH-1097 | Urgent | Backlog | RD-M9-008~013 Implement Global Search DTO/UI, keyboard navigation, task jump, Chinese fallback and incremental updates |
| INH-1098 | Urgent | Backlog | RD-M9-014~019 Implement Today, Upcoming, Overdue, Unscheduled, Completed and conditional Flagged Smart Views |
| INH-1099 | High | Backlog | RD-M9-020~021 Add Participant and Tag Smart Views |
| INH-1102 | High | Backlog | RD-M9-031~034 Finalize Command Registry and Ctrl+K palette |
| INH-1105 | High | Backlog | QA-M9-001~005 Global Search correctness, Chinese fallback and rebuild matrix |
| INH-1108 | High | Backlog | QA-M9-016 Run M0–M8 cumulative regression with productivity features enabled |
| INH-1109 | Urgent | Backlog | GATE-M9 Productivity Feature Complete Exit Gate |
| INH-1110 | High | Backlog | RD-M10-001 Build deterministic benchmark fixture generator |
| INH-1111 | High | Backlog | RD-M10-002~009 Generate canonical performance datasets |
| INH-1112 | High | Backlog | RD-M10-010 Define versioned benchmark result JSON and human report |
| INH-1115 | Urgent | Backlog | RD-M10-022 Decide Task Tree virtualization from measured evidence |
| INH-1116 | High | Backlog | RD-M10-023 Implement Task Tree virtualization only when RD-M10-022 gate says IMPLEMENT |
| INH-1127 | Urgent | Backlog | QA-M10-D01~D10 Core task UX and canonical identity RC matrix |
| INH-1130 | Urgent | Backlog | QA-M10-G01~G10 Productivity search, views and Quick Add RC matrix |
| INH-1132 | Urgent | Backlog | RC-M10 Blocker review and 2.0 scope freeze |
| INH-1133 | Urgent | Backlog | GATE-M10 ModernToDoList 2.0 GA Exit Gate |

## Gates that cannot be auto-verified in this environment

- **QA-M10-H (INH-1131)** requires execution on clean Windows 10 and Windows 11 VMs with no network, no administrator rights, and no Node/Rust toolchain installed. That is a manual, out-of-band activity and cannot be automated from a development workstation. Until it is executed by a human and the results recorded, GATE-M10 must not be closed.
- **RC-M10 (INH-1132)** additionally requires freezing the commit, the version string and the release ZIP SHA-256. Those can only be frozen once the blocker count above reaches 0.
