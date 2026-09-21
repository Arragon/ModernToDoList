# ModernToDoList 2.0.0 — Release Notes

**Release date:** 2026-01-15
**Artifact:** `ModernToDoList-2.0.0-Portable-win-x64.zip` (+ `.sha256` sidecar)
**Generated from:** `src-tauri/src/release/notes.rs` → `release_notes_2_0_0()`

> This document mirrors the structured release notes in code. Edit
> `release_notes_2_0_0()` and re-render, or edit both consistently. The
> template for future releases is at the bottom of this file.

## Highlights

- Portable Windows app: unzip and run, no installer, no admin rights, no network required
- Strict byte-level compatibility with the legacy TDL XML format (M0 audit verified)
- SQLite-derived search index with delete-and-rebuild recovery — XML files stay the single source of truth
- Reproducible release ZIP with published SHA-256 (RD-M10-024)

## New Features

- Task relations: participants, dependencies with cycle detection, progress links, attachments (INH-1042..INH-1054)
- Rich text editor with HTML whitelist sanitizer and comments-type preservation (INH-1061..INH-1067)
- Multi-document workspace: cross-document copy/move transactions, file library, trash (INH-1075..INH-1094)
- Productivity: FTS5 global search with CJK fallback, smart views, saved views, command palette, quick add (INH-1095..INH-1109)
- Versioned Data directory migration with index-rebuild fallback (INH-1118)
- Crash diagnostics with privacy-safe bug-report export (path hashes only) (INH-1122)

## Improvements

- Portable path hardening: Unicode paths, >260-char paths, removable-drive letter changes, read-only EXE directory fallback (INH-1121)
- WebView2 runtime detection with structured errors and offline fixed-version policy (INH-1119)
- Antivirus false-positive investigation process, mitigation checklist and code-signing policy (INH-1120)
- Third-party license notices generated from Cargo.lock and package-lock.json (INH-1123)
- EXE version metadata (file version, product version, company) embedded via tauri.conf.json

## Fixes

- Atomic save: kill-before/mid-write recovery never truncates the source XML
- Watcher fingerprint checks reject stale external-edit conflicts
- Undo/redo coalescing for description edits no longer serializes per keystroke

## Breaking Changes

- None — TDL XML files written by 1.x remain fully readable and byte-compatible.

## Known Issues

- FTS5 unicode61 tokenizer does not segment Chinese; search falls back to LIKE substring matching for CJK queries
- Releases are not code-signed yet; SmartScreen may warn on first run (see docs/release/ANTIVIRUS_FALSE_POSITIVES.md)

## Installation (Portable)

- Download `ModernToDoList-2.0.0-Portable-win-x64.zip` and verify its SHA-256 against the `.sha256` sidecar file
- Extract to a writable folder (or a USB drive) and run `ModernToDoList.exe`
- All user data lives in the `Data/` folder next to the EXE; upgrading preserves it (see docs/release/MANUAL_UPDATE.md)
- Requires the Microsoft Edge WebView2 runtime — usually preinstalled on Windows 10/11 (see docs/release/WEBVIEW2_POLICY.md)

---

## Release Notes Template (for future versions)

Copy the block below (or render `ReleaseNotes::template(version, date)`), fill
every section, delete nothing — write "None." rather than removing a section,
so releases stay comparable.

```markdown
# ModernToDoList <MAJOR.MINOR.PATCH> — Release Notes

**Release date:** <YYYY-MM-DD>
**Artifact:** ModernToDoList-<version>-Portable-win-x64.zip (+ .sha256 sidecar)

## Highlights
- <3–5 bullets a user should read even if they skip everything else>

## New Features
- <summary> (<INH-xxxx>)

## Improvements
- <summary> (<INH-xxxx>)

## Fixes
- <summary>

## Breaking Changes
- None — <compatibility statement> (must explicitly state XML compatibility status)

## Known Issues
- <issue + workaround or tracking reference>

## Installation (Portable)
- Download the ZIP and verify its SHA-256 against the .sha256 sidecar
- Extract to a writable location and run ModernToDoList.exe
- User data lives in Data/ next to the EXE; see docs/release/MANUAL_UPDATE.md for upgrades
- Requires the WebView2 runtime; see docs/release/WEBVIEW2_POLICY.md
```

Checklist before publishing notes:
- [ ] Version and date match `release::version_metadata::VersionMetadata`
- [ ] Artifact name matches `release::packaging::portable_zip_name(version)`
- [ ] SHA-256 of the final ZIP recorded in the release channel
- [ ] Breaking Changes section explicitly addresses TDL XML compatibility
- [ ] Known Issues lists every open Blocker-classified issue or states none
