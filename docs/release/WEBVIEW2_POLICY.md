# WebView2 Runtime Policy (ModernToDoList 2.0)

**Task refs:** RD-M10-028~029 / INH-1119 · **Runtime code:** `src-tauri/src/platform/windows/webview2.rs` · **Status:** GA policy

ModernToDoList is a Tauri 2 app: the UI renders inside the Microsoft Edge
WebView2 runtime. The Portable ZIP does **not** bundle the runtime by
default; this document defines detection, the offline fixed-version policy,
and the user-facing fallback.

## 1. Startup detection (structured, not string-matched)

At startup the app calls `webview2::detect_runtime()`, which returns either a
`WebView2RuntimeInfo` or a typed `WebView2Error`:

| Error variant | Meaning | UI behavior |
|---|---|---|
| `RuntimeNotInstalled` | No Evergreen registry entry (or `pv = 0.0.0.0`) and no fixed-version pin | Blocking dialog with install instructions (below) |
| `VersionTooOld { found, minimum }` | Evergreen present but below `MINIMUM_VERSION` (1.0.1823.32) | Blocking dialog, same instructions |
| `FixedVersionIncomplete { path }` | `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` is set but lacks `msedgewebview2.exe` | Dialog explaining how to repair or unset the pin |
| `RegistryQueryFailed { reason }` | `reg.exe` could not be executed | Dialog with manual verification steps |
| `InvalidVersionString { raw }` | Registry reported an unparseable version | Dialog suggesting reinstall |
| `UnsupportedPlatform` | Not running on Windows | Developer-facing only |

Detection order:
1. `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` environment variable (fixed-version
   pin — authoritative; a broken pin is reported, never silently bypassed).
2. Evergreen registry lookup via `reg query` on
   `HKLM/HKCU \ SOFTWARE\[WOW6432Node\]Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` (`pv` value).

## 2. Offline / air-gapped machines: fixed-version package policy

For environments without internet access (common for the portable USB use
case), Microsoft's **fixed-version** runtime package is supported:

1. On a machine with internet, download the *fixed-version* binary cabinet
   from the official page (see §3), choosing **x64** and a specific version
   (e.g. `126.0.2592.68`).
2. Extract the cabinet; the result is a folder named
   `Microsoft.WebView2.FixedVersionRuntime.<version>.x64`.
3. Copy that folder to the portable drive, e.g. next to the app:
   `X:\ModernToDoList\WebView2Runtime\`.
4. Set the environment variable before launching (or ship a `启动.bat`):
   ```
   set WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=X:\ModernToDoList\WebView2Runtime\Microsoft.WebView2.FixedVersionRuntime.126.0.2592.68.x64
   ModernToDoList.exe
   ```

Policy rules:
- The fixed-version folder must be **complete** (`msedgewebview2.exe` present);
  detection fails with `FixedVersionIncomplete` otherwise.
- Fixed-version runtimes do **not** auto-update. Security patches require
  manually replacing the folder; re-run detection after any swap.
- The Portable ZIP itself never contains the runtime (license + size); only
  the policy and this document ship with it.
- User data stays in `Data/webview2/` inside the portable root regardless of
  which runtime channel is used (see `portable::resolve_webview2_udf`).

## 3. User-facing download instructions (fallback)

Shown verbatim in the error dialog (`webview2::user_instructions`):

> ModernToDoList requires the Microsoft Edge WebView2 Runtime.
>
> 1. On a computer with internet access, open:
>    https://developer.microsoft.com/en-us/microsoft-edge/webview2/
> 2. Under "Download the WebView2 Runtime", choose the **Evergreen Standalone
>    Installer** (x64).
> 3. Copy the installer to this computer and run it (the per-user installer
>    needs no administrator rights).
> 4. Start ModernToDoList again.
>
> Air-gapped machines: ask your IT administrator for the offline
> fixed-version package described in `docs/release/WEBVIEW2_POLICY.md`.

Windows 11 and current Windows 10 machines normally already have the runtime
preinstalled via Microsoft Edge.

## 4. Testing

`webview2.rs` unit tests cover version parsing/comparison, the minimum-version
policy, `reg query` output parsing, fixed-version folder validation (including
version extraction from the folder name), and that every error variant maps
to non-empty user instructions.
