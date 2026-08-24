[CmdletBinding()]
param(
    [ValidateSet("Release", "Debug")]
    [string]$Configuration = "Release",
    [switch]$RunTests
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$profileArgument = if ($Configuration -eq "Release") { " --release" } else { "" }
$profileDirectory = $Configuration.ToLowerInvariant()
$output = Join-Path $repository "dist/windows-x86_64"
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

if (-not $env:RACKFORGE_BUNDLED_PLUGIN) {
    $bundled = Join-Path $repository "dist/bundled-plugins/RackForge-Concert-Grand.rfplugin"
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
    $env:RACKFORGE_BUNDLED_PLUGIN = $bundled
}

Push-Location $repository
try {
    & rustup run stable-x86_64-pc-windows-msvc rustc --version *> $null
    if ($LASTEXITCODE -ne 0) {
        & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
        if ($LASTEXITCODE -ne 0) { throw "Could not install the Windows MSVC Rust toolchain." }
    }
    if ($RunTests) {
        $tests = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && cargo +stable-x86_64-pc-windows-msvc test --locked -p rackforge-vst3' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName
        & $env:ComSpec /d /s /c $tests
        if ($LASTEXITCODE -ne 0) { throw "RackForge VST3 tests failed." }
    }
    $build = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && cargo +stable-x86_64-pc-windows-msvc build --locked{4} -p rackforge-vst3' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $profileArgument
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

Write-Host "RackForge VST3 bundle: $bundleBinary"
Write-Host "RackForge VST3 DLL:    $rawDll"
