# ModernToDoList 2.0 — Portable Build Script
# Usage: pwsh scripts/build-portable.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== Building ModernToDoList 2.0 Portable ===" -ForegroundColor Cyan

# Step 1: Build release
Write-Host "`n[1/4] Building release..." -ForegroundColor Yellow
cargo tauri build -- --bundles none

# Step 2: Collect artifacts
$releaseDir = "release\ModernToDoList-2.0.0-Portable-win-x64"
Write-Host "`n[2/4] Collecting artifacts to $releaseDir..." -ForegroundColor Yellow

New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item "src-tauri\target\release\ModernToDoList.exe" $releaseDir
New-Item -ItemType Directory -Force -Path "$releaseDir\Data" | Out-Null
New-Item -ItemType Directory -Force -Path "$releaseDir\LICENSES" | Out-Null

# Step 3: Copy licenses if available
Write-Host "`n[3/4] Copying licenses..." -ForegroundColor Yellow
if (Test-Path "LICENSES\*") {
    Copy-Item "LICENSES\*" "$releaseDir\LICENSES\" -Recurse
}

# Step 4: Package as zip
Write-Host "`n[4/4] Packaging portable zip..." -ForegroundColor Yellow
$zipPath = "release\ModernToDoList-2.0.0-Portable-win-x64.zip"
if (Test-Path $zipPath) {
    Remove-Item $zipPath -Force
}
Compress-Archive -Path "$releaseDir\*" -DestinationPath $zipPath

Write-Host "`n=== Build complete ===" -ForegroundColor Green
Write-Host "Portable: $zipPath" -ForegroundColor Cyan
