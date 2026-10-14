# Path Hardening (Windows Portable)

**Task refs:** RD-M10-030~033 / INH-1121 · **Code:** `src-tauri/src/platform/windows/pathhard.rs`, `portable.rs` · **Tests:** unit tests in both modules + `src-tauri/tests/m10_portable_tests.rs`

The portable app runs from arbitrary locations: USB sticks that change drive
letter, folders with Chinese names and spaces, deeply nested directories, and
write-protected media. This document records the four hardening guarantees and
how each is implemented and tested.

## 1. Removable drives and drive-letter changes (RD-M10-030)

**Problem.** A USB stick mounted as `E:` on Monday can be `G:` on Tuesday. Any
setting that stored an absolute path (`E:\TDL\Data\lists\work.xml`) breaks
silently, and the app may create a fresh empty `Data/` on the new letter,
making the user think their tasks are gone.

**Guarantee.** The installation is identified by *content*, not by letter.

- First run writes `PortableMarker` to `<portable_root>\.portable-id.json`:
  app name, a stable `install_id` UUID, creating version, RFC 3339 timestamp.
  `portable::init_portable_marker(version)` creates it once and never rewrites
  the id.
- On startup `portable::resolve_portable_root_hardened(remembered_install_id)`
  reads the marker next to the EXE. If the id matches, that root is used.
- If it does not match (EXE copied elsewhere, or the remembered root is now
  unreachable), `pathhard::mounted_drive_roots()` enumerates `A:\` … `Z:\` and
  `find_install_by_marker` scans them for the matching id. Missing marker →
  typed `MarkerNotFound`, never a silent fresh `Data/`.
- Markers are validated: corrupt JSON, a different `app` value, or an empty
  `install_id` are all rejected (`read_marker` → `None`).

**Stored paths.** Policy: anything persisted inside `Data/` (workspace roots,
document entries, recent files) is re-resolved at open time against the root
returned above; if a remembered absolute path is unreachable, the marker scan
supplies the new root and the path is rebased onto it. New persistence code
should store portable-root-relative paths wherever the format allows, so a
drive-letter change needs no data migration.

**Test.** `marker_survives_drive_letter_change_simulation` writes a marker in
one temp root, proves a second root without a marker does not match, then
proves the scan finds the install after the marker "reappears" at a new root,
and that a stale remembered root falls through to the scan.

## 2. Chinese characters and spaces (RD-M10-031)

**Guarantee.** Full Unicode path support with no ANSI/8.3 fallback anywhere.

- Every API in `pathhard.rs` takes `&Path`/`&PathBuf`. On Windows Rust passes
  these to the `*W` (UTF-16) Win32 APIs, so CJK names and spaces never round-
  trip through a code page.
- No path is ever put through `to_string_lossy()` for a filesystem operation;
  lossy conversion appears only in log messages and in `hash_path` input.
- `path_roundtrips_lossless(path)` asserts `Path → &str → Path` is lossless and
  is used by tests over CJK directory names.
- `has_windows_unsafe_components(path)` rejects control characters, the
  reserved `<>:"/\|?*` set and Win32 device names (`CON`, `NUL`, `COM1`, …) so
  a user-typed workspace path cannot produce an unwritable location. Note this
  must not reject CJK: it is a denylist, not an allowlist.
- The ZIP layer sets the UTF-8 filename flag (general purpose bit 11) in
  `scripts/release/make-portable-zip.cjs`, and the Rust packager writes entry
  names as UTF-8, so archives containing CJK paths extract correctly.

**Tests.** `unicode_paths_roundtrip_losslessly` and
`unicode_tempdir()` run real file I/O in temp directories named
`现代待办 测试 dir <uuid>`; `unsafe_components_are_detected` proves CJK names
are accepted while `CON`/`bad:name`/control chars are rejected. Crash-log
privacy tests additionally prove CJK paths never leak into diagnostics
(RD-M10-035).

## 3. Long paths beyond 260 characters (RD-M10-032)

**Guarantee.** Paths at or beyond `MAX_PATH` keep working.

- `extended_path(path)` adds the Win32 extended-length prefix when the path
  reaches `MAX_PATH - 12` (the 12-char margin Win32 reserves for 8.3 name
  generation):
  - `C:\a\b` → `\\?\C:\a\b`
  - `\\server\share\f` → `\\?\UNC\server\share\f`
  - already-prefixed and relative paths are returned unchanged.
- `extended_path_always(path)` prefixes unconditionally for callers that want
  it regardless of length; both are no-ops off Windows.
- `exceeds_max_path(path)` counts **characters**, not bytes, so a 100-character
  CJK path is not misreported as long.
- Used by `is_dir_writable`, `write_marker`, `read_marker` and the
  read-only-directory fallback, i.e. every place the portable layer touches
  the filesystem.
- Caveat documented for the orchestrator: the `\\?\` prefix disables Win32 path
  normalization, so callers must pass absolute, `.`/`..`-free paths. Relative
  paths are resolved against the portable root before prefixing.

**Test.** `create_and_read_file_beyond_260_chars` builds a real 12-level nested
CJK directory chain (well past 260 characters), then creates, writes and reads
back a file at the leaf.

## 4. Read-only EXE directory (RD-M10-033)

**Guarantee.** `Data/` is always writable; the app never dies on a
write-protected location.

`pathhard::ensure_writable_data_dir(exe_dir)` tries, in order:

1. `<exe_dir>\Data` — normal portable mode.
2. `%LOCALAPPDATA%\ModernToDoList\Data` — EXE directory read-only (running from
   `C:\Program Files` without admin, or a write-protected USB stick).
3. `%USERPROFILE%\.ModernToDoList\Data` — `LOCALAPPDATA` unavailable.

Writability is *probed*, not inferred from attributes: `is_dir_writable`
creates the directory, writes and removes a per-PID probe file
(`.mtl_write_probe_<pid>`). That catches ACL denials, read-only media and the
"Data exists as a file" corruption case, none of which a `FILE_ATTRIBUTE_READONLY`
check would see. Probe files are removed on both paths so no residue is left.

The result is a `DataDirLocation` enum — `Portable(path)` or
`RelocatedToProfile(path)` — so the caller can tell the user once that data
moved, instead of silently splitting state. `portable::ensure_data_dir_hardened()`
is the wrapper the startup path should call; the pre-existing
`portable::ensure_data_dir()` is unchanged and still returns an error rather
than falling back, for callers that must not relocate.

**Relocation is a last resort, not a preference.** Order matters: the portable
root is tried first every launch, so inserting a writable stick again returns
the user to portable mode.

**Tests.** `writable_dir_detection_and_relocation_fallback` asserts the normal
case returns `Portable(<exe_dir>/Data)` and that a blocked `Data` (a *file* at
that path) returns a relocated, genuinely writable directory;
`is_dir_writable_false_for_file_path` covers the probe itself.

## 5. Known limitations

- `mounted_drive_roots()` enumerates drive letters only; a portable install on
  a UNC share is found only if that share is the remembered root. UNC support
  for the *index* database is already handled by `db.rs` falling back from WAL
  to DELETE journaling.
- The marker scan is O(mounted drives) directory reads at startup. It runs only
  when the EXE-adjacent marker does not match, so the normal case costs one
  file read.
- Relocated mode means `Data/` no longer travels with the stick. The UI must
  surface this; the enum exists precisely so it can.
