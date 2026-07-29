[CmdletBinding()]
param(
    [string]$OutputRoot = (Join-Path $PSScriptRoot "..\tmp\display-hook"),
    [switch]$BuildRejectedPackageForAnalysis,
    [switch]$BuildArchivedInstalledPackageForAnalysis,
    [switch]$BuildArchivedRejectedRawSysexPackageForAnalysis
)

$ErrorActionPreference = "Stop"
$hookRoot = Join-Path $PSScriptRoot "hook"
$stock = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3.bin"
$output = [System.IO.Path]::GetFullPath($OutputRoot)
$target = Join-Path $hookRoot "target\thumbv7em-none-eabihf\release\rackforge-display-hook"
$hookSite = Join-Path $output "hook-site.bin"
$hookCode = Join-Path $output "hook-code.bin"
$candidate = Join-Path $output "keylab-essential-61-mk3-rackforge-display-candidate.bin.rejected-do-not-flash"
$manifest = Join-Path $output "manifest.json"
$officialPackage = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3"
$candidatePackage = Join-Path $output "keylab-essential-61-mk3-rackforge-display-candidate.kle3.rejected-do-not-flash"
$hardwarePackage = Join-Path $output "keylab-essential-61-mk3-rackforge-display-hook.installed-once-ineffective.archived"
$packageManifest = Join-Path $output "package-manifest.json"
$hardwarePackageManifest = Join-Path $output "hardware-package-manifest.json"
$rawSysexPackage = Join-Path $output "keylab-essential-61-mk3-rackforge-raw-sysex-hook.installed-once-broke-midi.archived"
$rawSysexPackageManifest = Join-Path $output "raw-sysex-package-manifest.json"
$llvmRoot = Join-Path (rustc --print sysroot) "lib\rustlib\x86_64-pc-windows-gnu\bin"
$objcopy = Join-Path $llvmRoot "llvm-objcopy.exe"
$objdump = Join-Path $llvmRoot "llvm-objdump.exe"
$readobj = Join-Path $llvmRoot "llvm-readobj.exe"

if (-not (Test-Path -LiteralPath $objcopy)) {
    throw "No se encontró llvm-objcopy en el toolchain Rust."
}

New-Item -ItemType Directory -Force -Path $output | Out-Null

Push-Location $hookRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo compilar el hook Thumb."
    }
}
finally {
    Pop-Location
}

& $readobj --relocations $target
if ($LASTEXITCODE -ne 0) {
    throw "No se pudieron validar las relocaciones del hook."
}

$disassembly = (& $objdump -d --triple=thumbv7em-none-eabihf $target) -join "`n"
if ($LASTEXITCODE -ne 0) {
    throw "No se pudo desensamblar el hook."
}
if ($disassembly -notmatch "0800b802 <rackforge_hook_site>") {
    throw "El hook no quedó enlazado al callback SysEx esperado."
}
if ($disassembly -notmatch "0x0800b7ed" -or $disassembly -notmatch "0x0800b809") {
    throw "La delegación oficial o su dirección de retorno no quedaron fijadas."
}
if ($disassembly -match "0x80423a2") {
    throw "Se detectó el antiguo branch relativo inválido."
}

& $objcopy "--dump-section=.hook_site=$hookSite" $target
if ($LASTEXITCODE -ne 0) {
    throw "No se pudo extraer el branch del hook."
}
& $objcopy "--dump-section=.text=$hookCode" $target
if ($LASTEXITCODE -ne 0) {
    throw "No se pudo extraer el código del hook."
}

python (Join-Path $PSScriptRoot "patch_firmware.py") `
    --stock $stock `
    --hook-site $hookSite `
    --hook-code $hookCode `
    --output $candidate `
    --manifest $manifest
if ($LASTEXITCODE -ne 0) {
    throw "El generador rechazó la candidata."
}

python -m unittest discover -s $PSScriptRoot -p "test_*.py"
if ($LASTEXITCODE -ne 0) {
    throw "Fallaron las pruebas del generador."
}

if ($BuildRejectedPackageForAnalysis) {
    python (Join-Path $PSScriptRoot "package_candidate.py") `
        --official-package $officialPackage `
        --candidate $candidate `
        --output $candidatePackage `
        --manifest $packageManifest
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo crear el contenedor rechazado para análisis."
    }
}

if ($BuildArchivedInstalledPackageForAnalysis) {
    python (Join-Path $PSScriptRoot "package_candidate.py") `
        --official-package $officialPackage `
        --candidate $candidate `
        --output $hardwarePackage `
        --manifest $hardwarePackageManifest `
        --status "INSTALLED_ONCE_INEFFECTIVE_ARCHIVE_ONLY"
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo crear el paquete de prueba física del hook."
    }
}

if ($BuildArchivedRejectedRawSysexPackageForAnalysis) {
    python (Join-Path $PSScriptRoot "package_candidate.py") `
        --official-package $officialPackage `
        --candidate $candidate `
        --output $rawSysexPackage `
        --manifest $rawSysexPackageManifest `
        --status "INSTALLED_ONCE_BROKE_MIDI_ARCHIVE_ONLY"
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo archivar el paquete rechazado del callback SysEx."
    }
}

Write-Output "candidate=$candidate"
Write-Output "manifest=$manifest"
if ($BuildRejectedPackageForAnalysis) {
    Write-Output "rejected_package=$candidatePackage"
    Write-Output "package_manifest=$packageManifest"
}
if ($BuildArchivedInstalledPackageForAnalysis) {
    Write-Warning "Paquete archivado: el hook fue ineficaz y no debe reinstalarse."
    Write-Output "archived_installed_package=$hardwarePackage"
    Write-Output "archived_installed_package_manifest=$hardwarePackageManifest"
}
if ($BuildArchivedRejectedRawSysexPackageForAnalysis) {
    Write-Warning "Archivo no instalable: este hook anuló MIDI y no debe repetirse."
    Write-Output "archived_rejected_raw_sysex_package=$rawSysexPackage"
    Write-Output "archived_rejected_raw_sysex_manifest=$rawSysexPackageManifest"
}
