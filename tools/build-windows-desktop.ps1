[CmdletBinding()]
param(
    [string]$OutputDirectory = "dist/windows-x86_64"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$runtime = $repository
$output = Join-Path $repository $OutputDirectory
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
if (-not (Test-Path -LiteralPath $vcvars)) {
    throw "vcvars64.bat was not found at $vcvars"
}

$clang = Join-Path $llvmBin "clang.exe"
$libclang = Join-Path $llvmBin "libclang.dll"
if (-not (Test-Path -LiteralPath $clang) -or -not (Test-Path -LiteralPath $libclang)) {
    throw "LLVM/Clang was not found at $llvmBin. Install LLVM (winget install LLVM.LLVM) to build RackForge with ASIO support."
}

Push-Location $runtime
try {
    & rustup run stable-x86_64-pc-windows-msvc rustc --version *> $null
    if ($LASTEXITCODE -ne 0) {
        & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
        if ($LASTEXITCODE -ne 0) {
            throw "Could not install the stable Windows MSVC Rust toolchain."
        }
    }

    $build = 'call "{0}" && set "PATH={1};%PATH%" && set "LIBCLANG_PATH={1}" && set "RUSTFLAGS=-C target-feature=+crt-static" && cargo +stable-x86_64-pc-windows-msvc build --locked --release -p rackforge-desktop' -f $vcvars, $llvmBin
    & $env:ComSpec /d /s /c $build
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge Desktop build failed."
    }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $output | Out-Null
$source = Join-Path $runtime "target/x86_64-pc-windows-msvc/release/rackforge-desktop.exe"
if (-not (Test-Path -LiteralPath $source)) {
    # Cargo may use the shared target directory without an explicit target
    # segment when the requested toolchain target is also its host target.
    $source = Join-Path $runtime "target/release/rackforge-desktop.exe"
}
Copy-Item -LiteralPath $source -Destination (Join-Path $output "RackForge.exe") -Force
Write-Host "RackForge Desktop: $(Join-Path $output 'RackForge.exe')"
