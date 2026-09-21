# Antivirus False Positives — Investigation, Mitigation and Code Signing

**Task ref:** RD-M10-034 / INH-1120 · **Machine-readable form:** `src-tauri/src/diagnostics/mod.rs` → `antivirus_false_positive_checklist()`

Tauri apps are small native binaries that spawn a WebView2 child process and
read/write files next to themselves. That combination trips heuristics built
for Electron packers, droppers and ransomware. This document is the process of
record; the Rust checklist struct exists so the app can surface the same items
to users without duplicating prose.

## 1. Why portable Tauri builds get flagged

| Trigger | Why it fires | Applies to us |
|---|---|---|
| No code signature | Unsigned PE with no reputation history; SmartScreen blocks on first sight | Yes, until signing lands |
| Low download volume | Reputation engines need install counts; a 2.0.0 GA has none | Yes at launch |
| WebView2 child process | Native EXE launching `msedgewebview2.exe` resembles a browser-hijack dropper | Yes |
| Writes beside the EXE | Portable `Data/` next to the binary matches ransomware drop patterns | Yes |
| Recursive directory walk | Workspace scan / index rebuild uses `walkdir` over user folders | Yes |
| `notify` file watcher | `ReadDirectoryChangesW` on many paths reads as mass enumeration | Yes |
| UPX / packed binary | Compressed sections defeat static scanning; near-universal heuristic hit | No — we do not pack |
| Installer that runs on extract | Self-extracting archives are the classic dropper shape | No — plain ZIP |
| Mutex + autostart keys | Single-instance mutex is fine; autostart would be persistence | Mutex only |

## 2. Investigation process

Follow in order. Do not skip step 2 — a tampered download is not a false
positive and must be treated as a security incident.

1. **Capture the report.** AV product name and version, detection name (e.g.
   `HEUR:BCKD.Win32.Generic`), the file path flagged, and the exact user action.
2. **Verify the hash.** Compare the user's ZIP/EXE SHA-256 against the
   published `.sha256` sidecar for that release. Mismatch → stop, treat as
   tampering, tell the user to re-download.
3. **Look it up.** Submit the hash to VirusTotal and the vendor's telemetry
   portal to see which engines flag it and under what name.
4. **Reproduce.** Clean Windows 10 and Windows 11 VMs, the same AV at default
   settings, no network for the portable path. Record whether the block is
   *static* (on-extract/on-write signature hit) or *behavioral* (during
   workspace scan or index rebuild).
5. **Classify.** Static → signing/reputation problem. Behavioral → scope
   problem; check whether we touch anything outside the portable root and the
   user-selected workspace (see PATH_HARDENING.md §3).
6. **Report upstream.** Vendor false-positive submission with: hash, detection
   name, download URL, description of legitimate behavior, and a link to this
   document.
7. **Track.** Record the vendor case ID in the Linear issue, and keep it open
   until the detection is withdrawn in a shipped definition update. Verify by
   re-running step 4 with updated definitions.

## 3. Mitigation checklist

| ID | Item | Severity | Phase |
|---|---|---|---|
| AV-01 | Publish SHA-256 sidecar for every artifact | required | release |
| AV-02 | Code-sign the EXE (see §4) | required | release |
| AV-03 | Never pack/compress the binary (no UPX) | required | build |
| AV-04 | Keep builds deterministic so one hash maps to one reputation | recommended | build |
| AV-05 | Submit the signed RC to Microsoft + top consumer vendors pre-release | recommended | rc |
| AV-06 | Document user-side exclusion steps and the verification hash | recommended | docs |
| AV-07 | Restrict runtime filesystem scope to the portable root and user-selected paths | recommended | code |
| AV-08 | Ship `README-PORTABLE.txt` inside the ZIP describing what the app writes | optional | release |

These eight items are exactly what `antivirus_false_positive_checklist()`
returns, with `id`, `title`, `detail`, `severity` and `phase` fields, so the
UI can render the checklist or attach it to a bug-report bundle.

**User-facing workaround text** (for the support page and AV-06):

> Some antivirus products flag new, rarely-downloaded Windows apps. Verify the
> download first: right-click the ZIP → Properties, and compare the SHA-256 in
> the `.sha256` file with the value published on the release page. If it
> matches, the file is authentic; add an exclusion for the extracted folder, or
> choose "Allow" in the SmartScreen prompt. Report the detection to us with
> your AV product name and version so we can file a false-positive report.

## 4. Code-signing policy

- **Signing is mandatory for GA.** Unsigned binaries are internal/CI builds
  only and must never be published as a release artifact.
- **Certificate type.** OV Authenticode at minimum; EV where budget allows,
  because EV earns SmartScreen reputation immediately rather than after
  download volume accumulates.
- **Timestamping.** Every signature must carry an RFC 3161 timestamp so it
  stays valid after the certificate expires.
- **Key custody.** Private keys live in an HSM or a cloud signing service.
  Never in the repository, never on a build agent's local disk, never in CI
  environment variables as raw key material.
- **Identity match.** The certificate subject must match the `company` field of
  `release::version_metadata::VersionMetadata` (`ModernToDoList Team`) and the
  `bundle.windows.publisher` value in `tauri.conf.json`, otherwise Explorer
  shows two different publisher names.
- **What gets signed.** `ModernToDoList.exe` inside the ZIP. The ZIP itself is
  not signed (hash sidecar covers integrity).
- **Compromise.** Revoke the certificate, re-sign affected releases with a new
  one, notify users in release notes, and re-run §2 step 7 for each vendor.
- **Interim state.** While unsigned, every release page and `MANUAL_UPDATE.md`
  must carry the SmartScreen warning plus the SHA-256 verification step.

## 5. Verification

`diagnostics::tests` assert the checklist is complete (≥6 mitigations, unique
IDs, non-empty fields) and serializes to camelCase JSON with lowercase
severity values, which is the contract the frontend consumes.
