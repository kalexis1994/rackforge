[CmdletBinding()]
param(
    [ValidateSet("Release", "Debug")]
    [string]$Configuration = "Release",
    [ValidateSet("Standard", "Minimal")]
    [string]$Edition = "Standard",
    [string]$OutputDirectory = "dist/windows-x86_64",
    [switch]$RunTests
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$profileArgument = if ($Configuration -eq "Release") { " --release" } else { "" }
$profileDirectory = $Configuration.ToLowerInvariant()
$output = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory
} else {
    Join-Path $repository $OutputDirectory
}
$rawDll = Join-Path $output "rackforge-vst3.dll"
$bundleBinary = Join-Path $output "RackForge.vst3/Contents/x86_64-win/RackForge.vst3"
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
$llvmBin = Join-Path $env:ProgramFiles "LLVM/bin"

if (-not (Test-Path -LiteralPath $vswhere)) {
    throw "Visual Studio Installer was not found. Install Visual Studio 2022 Build Tools with the C++ toolchain."
}
$visualStudio = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
if (-not $visualStudio) {
    throw "Visual Studio C++ Build Tools were not found."
}
$vcvars = Join-Path $visualStudio "VC/Auxiliary/Build/vcvars64.bat"
$msvcLinker = Get-ChildItem -Path (Join-Path $visualStudio "VC/Tools/MSVC") `
    -Filter "link.exe" -File -Recurse |
    Where-Object { $_.FullName -match '[\\/]bin[\\/]Hostx64[\\/]x64[\\/]link\.exe$' } |
    Sort-Object -Property {
        try { [version]$_.Directory.Parent.Parent.Parent.Name } catch { [version]"0.0" }
    } -Descending |
    Select-Object -First 1
if (-not $msvcLinker) {
    throw "The Visual Studio x64 MSVC linker was not found."
}
$msvcBin = $msvcLinker.DirectoryName
$libclang = Join-Path $llvmBin "libclang.dll"
if (-not (Test-Path -LiteralPath $libclang)) {
    throw "LLVM was not found at $llvmBin."
}

$defaultPlugin = ""
$rf106 = ""
if ($Edition -eq "Standard" -and -not $env:RACKFORGE_BUNDLED_PLUGIN) {
    $bundled = Join-Path $repository "dist/bundled-plugins/RF-Concert-Grand.rfplugin"
    if (-not (Test-Path -LiteralPath $bundled -PathType Leaf)) {
        Push-Location $repository
        try {
            & rustup target add wasm32-unknown-unknown
            if ($LASTEXITCODE -ne 0) { throw "Could not install the WebAssembly Rust target." }
            & cargo build --release --target wasm32-unknown-unknown -p rackforge-concert-grand
            if ($LASTEXITCODE -ne 0) { throw "Concert Grand WebAssembly build failed." }
            New-Item -ItemType Directory -Force (Split-Path -Parent $bundled) | Out-Null
            & cargo run --release -p rackforge-store -- pack-wasm `
                plugins/concert-grand/package `
                target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm `
                $bundled
            if ($LASTEXITCODE -ne 0) { throw "Concert Grand package build failed." }
        } finally {
            Pop-Location
        }
    }
    $defaultPlugin = $bundled
} elseif ($Edition -eq "Standard") {
    $defaultPlugin = $env:RACKFORGE_BUNDLED_PLUGIN
}

if ($Edition -eq "Standard") {
    $officialPlugins = Join-Path $repository "dist/bundled-plugins/official"
    & python (Join-Path $repository "tools/fetch-official-plugins.py") `
        --output-directory $officialPlugins
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge official plugin download failed."
    }
    $carried = @(Get-ChildItem -LiteralPath $officialPlugins -Filter *.rfplugin -File -ErrorAction SilentlyContinue)
    if ($carried.Count -eq 0) {
        throw "No pinned official plugins were produced in $officialPlugins"
    }
    Write-Host ("   embedding official instruments: " + (($carried | ForEach-Object { $_.BaseName }) -join ", "))
}

# The plug-in reads the directory rather than one named package, so a new
# official instrument needs no change here. Minimal is not handed the
# directory at all: what it does not receive it cannot carry.
$bundledEnvironment = if ($Edition -eq "Standard") {
    'set "RACKFORGE_EDITION={0}" && set "RACKFORGE_BUNDLED_PLUGIN={1}" && set "RACKFORGE_BUNDLED_OFFICIAL_PLUGINS={2}" &&' -f `
        $Edition.ToLowerInvariant(), $defaultPlugin, $officialPlugins
} else {
    'set "RACKFORGE_EDITION={0}" &&' -f $Edition.ToLowerInvariant()
}

Push-Location $repository
try {
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
        throw "pnpm is required to build the shared RackForge interface."
    }
    & pnpm --dir web build
    if ($LASTEXITCODE -ne 0) { throw "The shared RackForge interface build failed." }

    & rustup run stable-x86_64-pc-windows-msvc rustc --version *> $null
    if ($LASTEXITCODE -ne 0) {
        & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
        if ($LASTEXITCODE -ne 0) { throw "Could not install the Windows MSVC Rust toolchain." }
    }
    if ($RunTests) {
        $tests = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && {4} cargo +stable-x86_64-pc-windows-msvc test --locked -p rackforge-vst3' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $bundledEnvironment
        & $env:ComSpec /d /s /c $tests
        if ($LASTEXITCODE -ne 0) { throw "RackForge VST3 tests failed." }
    }
    $build = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && {4} cargo +stable-x86_64-pc-windows-msvc build --locked{5} -p rackforge-vst3' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $bundledEnvironment, $profileArgument
    & $env:ComSpec /d /s /c $build
    if ($LASTEXITCODE -ne 0) { throw "RackForge VST3 build failed." }
} finally {
    Pop-Location
}

$source = Join-Path $repository "target/x86_64-pc-windows-msvc/$profileDirectory/rackforge_vst3.dll"
if (-not (Test-Path -LiteralPath $source)) {
    $source = Join-Path $repository "target/$profileDirectory/rackforge_vst3.dll"
}
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "RackForge VST3 was not produced at $source"
}
New-Item -ItemType Directory -Force $output | Out-Null
New-Item -ItemType Directory -Force (Split-Path -Parent $bundleBinary) | Out-Null
try {
    Copy-Item -LiteralPath $source -Destination $rawDll -Force
    Copy-Item -LiteralPath $source -Destination $bundleBinary -Force
} catch {
    throw "Could not replace RackForge VST3. Close any DAW that has RackForge loaded and retry. $($_.Exception.Message)"
}
Copy-Item -LiteralPath (Join-Path $repository "THIRD_PARTY_NOTICES.md") `
    -Destination (Join-Path $output "THIRD_PARTY_NOTICES.md") -Force
$bundledPluginNames = @()
if ($defaultPlugin) {
    $bundledPluginNames += Split-Path -Leaf $defaultPlugin
}
if ($rf106) {
    $bundledPluginNames += Split-Path -Leaf $rf106
}
[System.IO.File]::WriteAllLines(
    (Join-Path $output "bundled-plugins.txt"),
    [string[]]$bundledPluginNames
)
"edition=$($Edition.ToLowerInvariant())" | Set-Content `
    -LiteralPath (Join-Path $output "build-info.txt") -Encoding utf8NoBOM

Write-Host "RackForge VST3 ($Edition) bundle: $bundleBinary"
Write-Host "RackForge VST3 ($Edition) DLL:    $rawDll"
