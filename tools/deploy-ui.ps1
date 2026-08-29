<#
.SYNOPSIS
One build of the shared interface, delivered to every host.

The platform drift of 2026-08-19 traced to the SAME SPA being distributed
three ways by hand. This script is the single path: build the dist once
(revision-stamped), then push it everywhere that serves it.

  ./tools/deploy-ui.ps1                # desktop + Raspberry Pi
  ./tools/deploy-ui.ps1 -Android      # also rebuild + install the APK
  ./tools/deploy-ui.ps1 -SkipPi       # desktop only

Desktop embeds the dist at compile time, so the exe is rebuilt. The Pi
receives the dist over SSH into its web root (rackforge-web serves files
from disk; no restart needed). Android bundles the dist inside the APK via
build-android.ps1, which already copies web/dist.
#>
[CmdletBinding()]
param(
    [switch]$Android,
    [switch]$SkipPi,
    [switch]$SkipDesktop,
    [string]$PiHost = "rackforge-pi",
    [string]$AndroidSerial = "192.168.1.93:38057"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
Set-Location $repository

Write-Host "== Building the shared interface (revision-stamped)" -ForegroundColor Cyan
pnpm --dir web build
if ($LASTEXITCODE -ne 0) { throw "web build failed" }
$revision = (Get-Content web/dist/ui-revision.txt).Trim()
Write-Host "   ui revision: $revision"

if (-not $SkipDesktop) {
    Write-Host "== Desktop: rebuilding the exe (embeds the dist)" -ForegroundColor Cyan
    $running = Get-Process rackforge-desktop -ErrorAction SilentlyContinue
    if ($running) {
        Write-Warning "rackforge-desktop is running; stop it and re-run, or the exe cannot be replaced."
    }
    cargo +stable-x86_64-pc-windows-msvc build --release -p rackforge-desktop
    if ($LASTEXITCODE -ne 0) { throw "desktop build failed" }
    # The desktop adopts the KeyLab .rfcontroller from the driver exe sitting
    # next to it; without this build the installed package silently rots on
    # old wire schemas (deny_unknown_fields breaks its LIVE menu).
    cargo +stable-x86_64-pc-windows-msvc build --release --manifest-path hardware/keylab-bridge/Cargo.toml --bin rackforge-arturia-keylab-essential-mk3-driver
    if ($LASTEXITCODE -ne 0) { throw "controller driver build failed" }
}

if (-not $SkipPi) {
    Write-Host "== Raspberry Pi: syncing the dist to the web root" -ForegroundColor Cyan
    tar -czf - -C web/dist . | ssh -o BatchMode=yes $PiHost `
        "rm -rf ~/rackforge/web.next && mkdir -p ~/rackforge/web.next && tar -xzf - -C ~/rackforge/web.next && rm -rf ~/rackforge/web.previous && mv ~/rackforge/web ~/rackforge/web.previous && mv ~/rackforge/web.next ~/rackforge/web && cat ~/rackforge/web/ui-revision.txt"
    if ($LASTEXITCODE -ne 0) { throw "Pi sync failed" }
}

if ($Android) {
    Write-Host "== Android: rebuilding and installing the APK" -ForegroundColor Cyan
    ./tools/build-android.ps1 | Out-Null
    $adb = Join-Path $env:LOCALAPPDATA "RackForge/android-sdk/platform-tools/adb.exe"
    & $adb -s $AndroidSerial install -r dist/android/RackForge-debug.apk
    if ($LASTEXITCODE -ne 0) { throw "Android install failed (is the device connected?)" }
}

Write-Host "== Done. Every host now serves UI $revision" -ForegroundColor Green
Write-Host "   Verify per host: GET /api/v1/health -> revision + ui_revision."
