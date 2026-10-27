# m10-h-evidence.ps1 - QA-M10-H matrix evidence collector.
#
# Runs INSIDE the clean VM (Windows PowerShell 5.1, no extra runtimes needed).
# It does NOT decide PASS/FAIL: it captures the mechanical facts each H case
# asks for (environment, hashes, path behaviour, bundle leaks) into a JSON
# file that the human attaches to INH-1131 next to their own observations.
#
# Keep this file ASCII-only. Non-ASCII fixture paths are passed in as
# arguments so no encoding assumption is baked into the script itself.
#
# Examples
#   powershell -ExecutionPolicy Bypass -File m10-h-evidence.ps1 -Check env
#   ... -Check zip   -ZipPath C:\in\ModernToDoList-2.0.0-Portable-win-x64.zip `
#                    -SidecarPath C:\in\ModernToDoList-2.0.0-Portable-win-x64.zip.sha256
#   ... -Check paths -Workspace 'D:\<cjk>\<cjk>\my task''s list & notes.xml'
#   ... -Check crash -BundleDir C:\Users\me\Desktop\crash-bundle -Forbidden 'buy milk','<cjk folder>'
#   ... -Check index -AppDir 'C:\Portable\ModernToDoList'

[CmdletBinding()]
param(
  [ValidateSet('env', 'zip', 'paths', 'crash', 'index')]
  [string]$Check = 'env',

  [string]$ZipPath,
  [string]$SidecarPath,
  [string]$Workspace,
  [string]$AppDir,
  [string]$BundleDir,
  [string[]]$Forbidden,

  [string]$OutDir,
  [switch]$RemoveIndex
)

$ErrorActionPreference = 'Stop'
$script:results = New-Object System.Collections.ArrayList

# `-File` invocation does not split comma-separated arguments, so normalise here:
#   -Forbidden 'buy milk;<cjk folder>'  and  -Forbidden 'a','b'  both work.
if ($Forbidden) {
  $Forbidden = @($Forbidden | ForEach-Object { $_ -split '[;]' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}

function Add-Result {
  param([string]$Name, [object]$Value, [string]$Note = '')
  [void]$script:results.Add([pscustomobject]@{
    check = $Check; name = $Name; value = $Value; note = $Note
  })
  $shown = if ($null -eq $Value) { '<null>' } else { "$Value" }
  Write-Host ("  {0,-34} {1}" -f $Name, $shown)
}

function Get-RegValue {
  param([string]$Path, [string]$Name)
  if (-not (Test-Path $Path)) { return $null }
  try { return (Get-ItemProperty -Path $Path -Name $Name -ErrorAction Stop).$Name }
  catch { return $null }
}

# ---------------------------------------------------------------- env (H02-H04)
function Invoke-EnvCheck {
  $os = Get-CimInstance Win32_OperatingSystem
  Add-Result 'os_caption'    $os.Caption
  Add-Result 'os_version'    $os.Version
  Add-Result 'os_build'      $os.BuildNumber
  Add-Result 'os_architecture' $os.OSArchitecture
  Add-Result 'locale'        (Get-Culture).Name
  Add-Result 'ui_language'   (Get-UICulture).Name
  Add-Result 'powershell'    $PSVersionTable.PSVersion.ToString()

  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  $isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  Add-Result 'user' $identity.Name
  Add-Result 'is_admin' $isAdmin 'H04/H05 require False'
  Add-Result 'elevation_uac' (Get-RegValue 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' 'EnableLUA')

  # WebView2 must be ABSENT for H02 and PRESENT for the rest.
  $wv2Keys = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  )
  $wv2 = $null
  foreach ($k in $wv2Keys) {
    $v = Get-RegValue $k 'pv'
    if ($v -and $v -ne '0.0.0.0') { $wv2 = $v; break }
  }
  Add-Result 'webview2_version' $wv2 'null means runtime absent (H02 precondition)'
  Add-Result 'webview2_present' ([bool]$wv2)

  $net = @(Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { $_.Status -eq 'Up' } | Select-Object -ExpandProperty Name)
  Add-Result 'net_adapters_up' ($net -join '; ')
  Add-Result 'net_up_count' $net.Count 'H03 requires 0'

  # No toolchain on a clean VM.
  foreach ($exe in @('node', 'cargo', 'rustc', 'python', 'git')) {
    $cmd = Get-Command $exe -ErrorAction SilentlyContinue
    Add-Result ("tool_$exe") ($(if ($cmd) { $cmd.Source } else { $null }))
  }

  Add-Result 'data_dir_exists' (Test-Path (Join-Path $env:USERPROFILE 'ModernToDoList'))
  Add-Result 'program_files_write' (Test-Path $env:ProgramFiles)
}

# ---------------------------------------------------------------- zip (H01)
function Invoke-ZipCheck {
  if (-not $ZipPath)      { throw '-ZipPath is required for -Check zip' }
  if (-not (Test-Path $ZipPath)) { throw "zip not found: $ZipPath" }

  $actual = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
  $size = (Get-Item $ZipPath).Length
  Add-Result 'zip_path' $ZipPath
  Add-Result 'zip_bytes' $size 'frozen RC: 4107682'
  Add-Result 'zip_sha256' $actual

  $expected = $null
  if ($SidecarPath -and (Test-Path $SidecarPath)) {
    $line = (Get-Content -Path $SidecarPath -TotalCount 1).Trim()
    if ($line -match '^([0-9a-fA-F]{64})') { $expected = $Matches[1].ToLowerInvariant() }
  }
  Add-Result 'sidecar_sha256' $expected
  Add-Result 'hash_match' ($(if ($expected) { $actual -eq $expected } else { $null })) 'H01 PASS requires True'

  try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
    $entries = @($zip.Entries | ForEach-Object { "$($_.FullName) [$($_.Length)]" })
    $zip.Dispose()
    Add-Result 'zip_entry_count' $entries.Count 'frozen RC: 6'
    Add-Result 'zip_entries' ($entries -join ' | ')
  } catch {
    Add-Result 'zip_entries' $null "cannot enumerate: $($_.Exception.Message)"
  }
}

# ------------------------------------------------------------- paths (H08-H10)
function Invoke-PathsCheck {
  if (-not $Workspace) { throw '-Workspace is required for -Check paths' }

  $full = [System.IO.Path]::GetFullPath($Workspace)
  Add-Result 'workspace_target' $full
  Add-Result 'path_length' $full.Length 'H10 requires > 260'
  Add-Result 'exceeds_260' ($full.Length -gt 260)

  $dir = Split-Path -Parent $full
  try {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Add-Result 'dir_create' 'ok'
  } catch {
    Add-Result 'dir_create' "FAIL: $($_.Exception.Message)"
  }

  # Write the same payload the app writes: UTF-8 XML with an explicit declaration.
  $payload = '<?xml version="1.0" encoding="UTF-8"?><TODOLIST NEXTUNIQUEID="2"><TASK ID="1" TITLE="buy milk" /></TODOLIST>'
  try {
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($full, $payload, $utf8)
    $written = (Get-Item -LiteralPath $full).Length
    Add-Result 'write_plain_path' "ok ($written bytes)"
  } catch {
    Add-Result 'write_plain_path' "FAIL: $($_.Exception.Message)"
  }

  # Long-path prefix should succeed where the plain path may not.
  try {
    $lp = '\\?\' + $full
    $read = [System.IO.File]::ReadAllText($lp)
    Add-Result 'read_longpath_prefix' "ok ($($read.Length) chars)"
    Add-Result 'bytes_stable' ($read -eq $payload)
  } catch {
    Add-Result 'read_longpath_prefix' "FAIL: $($_.Exception.Message)"
  }

  $bytes = [System.IO.File]::ReadAllBytes($full)
  Add-Result 'file_sha256' ((Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant())
  Add-Result 'bom_present' ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)

  $enc = $null
  try {
    $first = (Get-Content -LiteralPath $full -TotalCount 1 -Encoding UTF8)
    if ($first -match 'encoding="([^"]+)"') { $enc = $Matches[1] }
  } catch { }
  Add-Result 'declared_encoding' $enc 'H08 requires the original declaration to survive'

  # Container check: can the app resolve its data directory next to a read-only exe?
  if ($AppDir) {
    Add-Result 'app_dir' $AppDir
    Add-Result 'app_dir_exists' (Test-Path $AppDir)
    Add-Result 'app_data_dir' (Join-Path $AppDir 'Data')
    Add-Result 'app_data_exists' (Test-Path (Join-Path $AppDir 'Data'))
  }
}

# ------------------------------------------------------------- crash (H13)
function Invoke-CrashCheck {
  if (-not $BundleDir)   { throw '-BundleDir is required for -Check crash' }
  if (-not (Test-Path $BundleDir)) { throw "bundle not found: $BundleDir" }

  $files = @(Get-ChildItem -LiteralPath $BundleDir -Recurse -File)
  Add-Result 'bundle_dir' $BundleDir
  Add-Result 'bundle_file_count' $files.Count
  Add-Result 'bundle_files' (($files | ForEach-Object { "$($_.Name) [$($_.Length)]" }) -join ' | ')

  if (-not $Forbidden -or $Forbidden.Count -eq 0) {
    Add-Result 'forbidden_scan' 'skipped (no -Forbidden values supplied)'
    return
  }

  # Privacy rule: task content and verbatim paths must never appear.
  foreach ($needle in $Forbidden) {
    $hits = 0
    $where = @()
    foreach ($f in $files) {
      try {
        $text = [System.IO.File]::ReadAllText($f.FullName)
      } catch { continue }
      if ($text.IndexOf($needle, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
        $hits++; $where += $f.Name
      }
    }
    Add-Result ("forbidden[$needle]") $hits 'H13 PASS requires 0 hits'
    if ($hits -gt 0) { Add-Result ("forbidden_files[$needle]") ($where -join ', ') }
  }
}

# ------------------------------------------------------------- index (H11)
function Invoke-IndexCheck {
  if (-not $AppDir) { throw '-AppDir is required for -Check index' }
  $dataDirs = @(
    (Join-Path $AppDir 'Data'),
    (Join-Path $env:USERPROFILE 'ModernToDoList')
  ) | Where-Object { Test-Path $_ }

  if ($dataDirs.Count -eq 0) {
    Add-Result 'data_dir' $null "no Data directory under $AppDir or the user profile; launch the app once first"
    return
  }

  foreach ($d in $dataDirs) {
    $db = Join-Path $d 'index.db'
    Add-Result "data_dir[$d]" 'found'
    Add-Result "index_db[$d]" ($(if (Test-Path $db) { (Get-Item $db).Length } else { 'absent' }))
    if (Test-Path $db) {
      Add-Result "index_sha256[$d]" ((Get-FileHash -LiteralPath $db -Algorithm SHA256).Hash.ToLowerInvariant())
    }
    $xmlCount = @(Get-ChildItem -LiteralPath $d -Recurse -Filter *.xml -ErrorAction SilentlyContinue)
    Add-Result "xml_files[$d]" $xmlCount.Count
    Add-Result "xml_sha256[$d]" (($xmlCount | ForEach-Object { "$($_.Name)=$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant().Substring(0,16))" }) -join ' | ')
  }

  if ($RemoveIndex) {
    $removed = @()
    foreach ($d in $dataDirs) {
      $db = Join-Path $d 'index.db'
      if (Test-Path $db) { Remove-Item -LiteralPath $db -Force; $removed += $db }
    }
    Add-Result 'index_removed' ($(if ($removed.Count) { $removed -join '; ' } else { 'nothing to remove' })) 'H11: relaunch now, then re-run with -Check index'
  }
}

# ------------------------------------------------------------------ main
switch ($Check) {
  'env'   { Invoke-EnvCheck }
  'zip'   { Invoke-ZipCheck }
  'paths' { Invoke-PathsCheck }
  'crash' { Invoke-CrashCheck }
  'index' { Invoke-IndexCheck }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $OutDir) { $OutDir = Join-Path $PSScriptRoot '..\..\docs\qa\evidence' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$outFile = Join-Path $OutDir ("{0}-{1}.h-evidence.json" -f $Check, $stamp)

$report = [pscustomobject]@{
  matrix      = 'QA-M10-H'
  issues      = 'INH-1131'
  gate        = 'GATE-M10 / INH-1133'
  check       = $Check
  captured_at = (Get-Date).ToString('o')
  host        = $env:COMPUTERNAME
  results     = $script:results
  caveat      = 'Mechanical facts only. PASS/FAIL stays a human judgement per docs/qa/QA_M10_H_PORTABLE_PROTOCOL.md.'
}
($report | ConvertTo-Json -Depth 6) | Set-Content -LiteralPath $outFile -Encoding UTF8

Write-Host ""
Write-Host "wrote $outFile"
Write-Host "attach this file to INH-1131 together with your own observation notes."
