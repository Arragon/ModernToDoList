# Manual Update Guide (Portable ZIP)

**Task ref:** RD-M10-039 / INH-1123 · Audience: end users and IT admins of the Portable build.

ModernToDoList 2.0 Portable has **no auto-updater** — by design. A portable
app on a USB stick must not silently download and replace itself. Updates are
a manual ZIP swap. Your data is never inside the program files, so the swap
cannot lose tasks.

## 1. What you need

- The new release ZIP, e.g. `ModernToDoList-2.0.0-Portable-win-x64.zip`
- Its `.sha256` sidecar file (published next to the ZIP)
- Your existing installation folder (the one containing `ModernToDoList.exe`)

## 2. Verify the download (do this first)

In Git Bash / WSL:

```
sha256sum ModernToDoList-2.0.0-Portable-win-x64.zip
```

In PowerShell:

```
Get-FileHash .\ModernToDoList-2.0.0-Portable-win-x64.zip -Algorithm SHA256
```

The printed value must equal the contents of the `.sha256` file. Because the
release ZIP is built deterministically (RD-M10-024), the hash published for a
version is identical for every mirror and every re-download. A mismatch means
a corrupted or tampered download — delete it and fetch again (see
`ANTIVIRUS_FALSE_POSITIVES.md` §2).

## 3. Update steps

1. **Close ModernToDoList.** Check the tray and Task Manager; a running
   instance holds `Data/index.db` open.
2. **Back up `Data/`.** Copy the whole folder somewhere safe. It holds
   `settings.json`, `lists/`, `logs/` and `index.db`. This costs seconds and
   makes every later step reversible.
3. **Note your install folder**, e.g. `E:\ModernToDoList\`.
4. **Extract the new ZIP to a temporary folder**, not over the install.
5. **Copy the program files over the install folder**, replacing them:
   `ModernToDoList.exe` and any bundled DLL/resource folders.
   **Do not delete or overwrite `Data/`.** The new ZIP does not contain a
   `Data/` folder; if your extraction tool offers to "merge", decline anything
   touching `Data/`.
6. **Start `ModernToDoList.exe`.** On first launch the migration framework
   (RD-M10-025~027) upgrades `Data/`:
   - `Data/version.json` is created/updated with the Data directory version and
     the settings schema version.
   - `Data/settings.json` gains `schemaVersion` and legacy flat keys are moved
     into sections (`theme` → `appearance.theme`,
     `autosave_secs` → `session.autosaveSecs`). Unknown keys are preserved.
   - `Data/lists/`, `Data/logs/` and `Data/webview2/` are created as needed.
   Migrations are sequential and idempotent, and each step is stamped, so an
   interrupted update resumes instead of restarting.
7. **Verify.** Open your usual task list, confirm tasks and comments look
   right, then check `Data/version.json` shows the expected version.
8. **Delete the temporary extraction folder** once satisfied. Keep the backup
   for a few days.

## 4. Downgrade

Restoring the backup `Data/` folder plus the older program files returns you to
the previous state. Note that a *newer* `Data/` directory is refused by an
older build (`FutureDataDir` / `FutureSchema` errors) — this is intentional,
because an older build cannot know how to interpret newer settings. Downgrade
by restoring the backup, not by pointing the old build at the new `Data/`.

## 5. When the index is broken or a migration failed

`Data/index.db` is a **derived** search index. XML/TDL files remain the source
of truth, so deleting it loses nothing but a rebuild.

- The app does this automatically: if a Data migration fails, the fallback
  deletes `index.db` (plus `-wal`/`-shm` sidecars) and rebuilds the index from
  the XML documents (RD-M10-027).
- Manually: close the app, delete `Data/index.db`, `Data/index.db-wal` and
  `Data/index.db-shm`, restart, then trigger a workspace re-scan.

## 6. Updating on a read-only or write-protected location

If the EXE directory cannot be written (read-only media, `C:\Program Files`
without admin), the app relocates `Data/` under your profile
(`%LOCALAPPDATA%\ModernToDoList\Data`) and reports it
(RD-M10-033 / `PATH_HARDENING.md` §4). In that case:

- Update the program files as above (admin rights may be needed).
- Your data is in the relocated folder, **not** next to the EXE. Back that up.
- If you move to a writable stick, the portable `Data/` next to the EXE takes
  precedence again on the next launch.

## 7. Drive letter changed after replugging a USB stick

Nothing to do. The install is identified by `.portable-id.json` at the portable
root, not by its letter, so the app finds its own `Data/` again
(`PATH_HARDENING.md` §1). If it reports that no installation was found, verify
`.portable-id.json` still exists at the root of the stick.

## 8. WebView2 runtime

If the new version requires a newer runtime, or you are on an air-gapped
machine, follow `WEBVIEW2_POLICY.md`. Evergreen runtimes update themselves with
Microsoft Edge; fixed-version folders must be replaced manually.

## 9. Update checklist (copy into your notes)

- [ ] App closed, no `ModernToDoList.exe` in Task Manager
- [ ] ZIP SHA-256 matches the published `.sha256`
- [ ] `Data/` backed up
- [ ] New program files copied in; `Data/` untouched
- [ ] App starts, `Data/version.json` updated, task list opens correctly
- [ ] Temp extraction folder removed; backup retained
