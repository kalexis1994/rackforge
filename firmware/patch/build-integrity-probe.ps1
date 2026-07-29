[CmdletBinding()]
param(
    [string]$OutputRoot = (Join-Path $PSScriptRoot "..\tmp\integrity-probe"),
    [switch]$BuildArchivedPackageForAnalysis
)

$ErrorActionPreference = "Stop"
$stock = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3.bin"
$output = [System.IO.Path]::GetFullPath($OutputRoot)
$candidate = Join-Path $output "keylab-essential-61-mk3-integrity-probe.bin.offline-not-tested"
$manifest = Join-Path $output "manifest.json"
$officialPackage = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3"
$candidatePackage = Join-Path $output "keylab-essential-61-mk3-integrity-probe.passed-on-hardware.archived"
$packageManifest = Join-Path $output "package-manifest.json"

New-Item -ItemType Directory -Force -Path $output | Out-Null

python (Join-Path $PSScriptRoot "integrity_probe.py") `
    --stock $stock `
    --output $candidate `
    --manifest $manifest
if ($LASTEXITCODE -ne 0) {
    throw "El generador rechazó la prueba de integridad."
}

python -m unittest discover -s $PSScriptRoot -p "test_*.py"
if ($LASTEXITCODE -ne 0) {
    throw "Fallaron las pruebas del generador."
}

if ($BuildArchivedPackageForAnalysis) {
    python (Join-Path $PSScriptRoot "package_candidate.py") `
        --official-package $officialPackage `
        --candidate $candidate `
        --output $candidatePackage `
        --manifest $packageManifest `
        --status "PASSED_ON_HARDWARE_ARCHIVE_ONLY"
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo crear el paquete de prueba de integridad."
    }
}

Write-Output "candidate=$candidate"
Write-Output "manifest=$manifest"
if ($BuildArchivedPackageForAnalysis) {
    Write-Warning "Paquete archivado después de una prueba exitosa. No instalar."
    Write-Output "archived_package=$candidatePackage"
    Write-Output "package_manifest=$packageManifest"
}
