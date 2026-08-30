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
$runtime = $repository
$output = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory
} else {
    Join-Path $repository $OutputDirectory
}
$outputExecutable = Join-Path $output "rackforge.exe"
$cargoProfileArgument = if ($Configuration -eq "Release") { " --release" } else { "" }
$cargoProfileDirectory = $Configuration.ToLowerInvariant()
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio/Installer/vswhere.exe"
$llvmBin = Join-Path $env:ProgramFiles "LLVM/bin"
$officialPlugins = ""
$defaultPlugin = ""

$runningRackForge = Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ProcessName -in @("rackforge", "rackforge-desktop")
}
if ($runningRackForge) {
    $processes = ($runningRackForge | ForEach-Object { "$($_.ProcessName) ($($_.Id))" }) -join ", "
    throw "RackForge is open ($processes). Close it before rebuilding $outputExecutable."
}

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

# Resolve the MSVC linker by absolute path. Git for Windows also ships a
# different program named link.exe, and hosted CI runners may put it before
# Visual Studio on PATH.
$msvcLinker = Get-ChildItem -Path (Join-Path $visualStudio "VC/Tools/MSVC") `
    -Filter "link.exe" -File -Recurse |
    Where-Object { $_.FullName -match '[\\/]bin[\\/]Hostx64[\\/]x64[\\/]link\.exe$' } |
    Sort-Object -Property {
        $versionDirectory = $_.Directory.Parent.Parent.Parent.Name
        try { [version]$versionDirectory } catch { [version]"0.0" }
    } -Descending |
    Select-Object -First 1
if (-not $msvcLinker) {
    throw "The Visual Studio x64 MSVC linker was not found below $visualStudio."
}
$msvcBin = $msvcLinker.DirectoryName

$clang = Join-Path $llvmBin "clang.exe"
$libclang = Join-Path $llvmBin "libclang.dll"
if (-not (Test-Path -LiteralPath $clang) -or -not (Test-Path -LiteralPath $libclang)) {
    throw "LLVM/Clang was not found at $llvmBin. Install LLVM (winget install LLVM.LLVM) to build RackForge with ASIO support."
}

if ($Edition -eq "Standard") {
    $officialPlugins = Join-Path $repository "dist/bundled-plugins/official"
    & python (Join-Path $repository "tools/fetch-official-plugins.py") `
        --output-directory $officialPlugins
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge official plugin download failed."
    }
    if ($env:RACKFORGE_BUNDLED_PLUGIN) {
        $defaultPlugin = $env:RACKFORGE_BUNDLED_PLUGIN
    } else {
        $localDefaultPlugin = Join-Path $repository `
            "dist/bundled-plugins/RackForge-Concert-Grand.rfplugin"
        if (Test-Path -LiteralPath $localDefaultPlugin -PathType Leaf) {
            $defaultPlugin = $localDefaultPlugin
        }
    }
}
$bundledEnvironment = 'set "RACKFORGE_EDITION={0}" && set "RACKFORGE_BUNDLED_PLUGIN={1}" && set "RACKFORGE_BUNDLED_OFFICIAL_PLUGINS={2}" &&' -f $Edition.ToLowerInvariant(), $defaultPlugin, $officialPlugins

Push-Location $runtime
try {
    # The executable embeds web/dist, so the interface is built here rather
    # than assumed. Trusting whatever happened to be lying in web/dist shipped
    # an executable stamped with today's revision carrying an interface from
    # several commits earlier — visible only because the health endpoint
    # publishes the two revisions separately. CI never saw it, because the
    # workflow builds the interface in a step of its own; a local build had
    # nothing to keep it honest.
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
        throw "pnpm is required to build the shared RackForge interface."
    }
    & pnpm --dir web build
    if ($LASTEXITCODE -ne 0) { throw "The shared RackForge interface build failed." }

    & rustup run stable-x86_64-pc-windows-msvc rustc --version *> $null
    if ($LASTEXITCODE -ne 0) {
        & rustup toolchain install stable-x86_64-pc-windows-msvc --profile minimal
        if ($LASTEXITCODE -ne 0) {
            throw "Could not install the stable Windows MSVC Rust toolchain."
        }
    }

    # Build the bundled controller first. Desktop embeds these exact bytes in
    # rackforge.exe, so startup can atomically install the matching immutable
    # .rfcontroller version without relying on a sidecar executable.
    $controllerBuild = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && cargo +stable-x86_64-pc-windows-msvc build --locked{4} -p rackforge-controller-arturia-keylab-essential-mk3 --bin rackforge-arturia-keylab-essential-mk3-driver' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $cargoProfileArgument
    & $env:ComSpec /d /s /c $controllerBuild
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge controller driver build failed."
    }
    $controllerSource = Join-Path $runtime "target/x86_64-pc-windows-msvc/$cargoProfileDirectory/rackforge-arturia-keylab-essential-mk3-driver.exe"
    if (-not (Test-Path -LiteralPath $controllerSource)) {
        $controllerSource = Join-Path $runtime "target/$cargoProfileDirectory/rackforge-arturia-keylab-essential-mk3-driver.exe"
    }
    if (-not (Test-Path -LiteralPath $controllerSource)) {
        throw "RackForge controller driver was not produced at $controllerSource"
    }

    if ($RunTests) {
        $tests = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && {4} set "RACKFORGE_BUNDLED_CONTROLLER_DRIVER={5}" && cargo +stable-x86_64-pc-windows-msvc test --locked -p rackforge-desktop' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $bundledEnvironment, $controllerSource
        & $env:ComSpec /d /s /c $tests
        if ($LASTEXITCODE -ne 0) {
            throw "RackForge Desktop tests failed."
        }
    }

    $build = 'call "{0}" && set "PATH={1};{2};%PATH%" && set "LIBCLANG_PATH={2}" && set "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER={3}" && set "RUSTFLAGS=-C target-feature=+crt-static" && {4} set "RACKFORGE_BUNDLED_CONTROLLER_DRIVER={5}" && cargo +stable-x86_64-pc-windows-msvc build --locked{6} -p rackforge-desktop' -f $vcvars, $msvcBin, $llvmBin, $msvcLinker.FullName, $bundledEnvironment, $controllerSource, $cargoProfileArgument
    & $env:ComSpec /d /s /c $build
    if ($LASTEXITCODE -ne 0) {
        throw "RackForge Desktop build failed."
    }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $output | Out-Null
$source = Join-Path $runtime "target/x86_64-pc-windows-msvc/$cargoProfileDirectory/rackforge-desktop.exe"
if (-not (Test-Path -LiteralPath $source)) {
    # Cargo may use the shared target directory without an explicit target
    # segment when the requested toolchain target is also its host target.
    $source = Join-Path $runtime "target/$cargoProfileDirectory/rackforge-desktop.exe"
}
if (-not (Test-Path -LiteralPath $source)) {
    throw "RackForge Desktop was not produced at $source"
}
Copy-Item -LiteralPath $source -Destination $outputExecutable -Force
Copy-Item -LiteralPath (Join-Path $repository "THIRD_PARTY_NOTICES.md") `
    -Destination (Join-Path $output "THIRD_PARTY_NOTICES.md") -Force
$bundledPluginNames = @()
if ($defaultPlugin) {
    $bundledPluginNames += Split-Path -Leaf $defaultPlugin
}
if ($officialPlugins) {
    $bundledPluginNames += Get-ChildItem -LiteralPath $officialPlugins `
        -Filter "*.rfplugin" -File | Sort-Object Name | Select-Object -ExpandProperty Name
}
[System.IO.File]::WriteAllLines(
    (Join-Path $output "bundled-plugins.txt"),
    [string[]]$bundledPluginNames
)
"edition=$($Edition.ToLowerInvariant())" | Set-Content `
    -LiteralPath (Join-Path $output "build-info.txt") -Encoding utf8NoBOM
Write-Host "RackForge Desktop ($Configuration, $Edition): $outputExecutable"
