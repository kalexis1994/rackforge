[CmdletBinding()]
param(
    [string]$OutputRoot = (Join-Path $PSScriptRoot "..\tmp\inert-diagnostic"),
    [switch]$BuildRejectedPackageForAnalysis
)

$ErrorActionPreference = "Stop"
$stock = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3.bin"
$officialPackage = Join-Path $PSScriptRoot "..\recovery\stock-official-1.2.1\keylab-essential-61-mk3_Firmware_Update_1.2.1.kle3"
$output = [System.IO.Path]::GetFullPath($OutputRoot)
$candidate = Join-Path $output "keylab-essential-61-mk3-inert-diagnostic.bin.offline-not-tested"
$manifest = Join-Path $output "manifest.json"
$candidatePackage = Join-Path $output "keylab-essential-61-mk3-inert-diagnostic.kle3.rejected-do-not-flash"
$packageManifest = Join-Path $output "package-manifest.json"

New-Item -ItemType Directory -Force -Path $output | Out-Null

python (Join-Path $PSScriptRoot "inert_candidate.py") `
    --stock $stock `
    --output $candidate `
    --manifest $manifest
if ($LASTEXITCODE -ne 0) {
    throw "El generador rechazó el diagnóstico inerte."
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
        throw "No se pudo crear el paquete del diagnóstico inerte."
    }
}

Write-Output "candidate=$candidate"
Write-Output "manifest=$manifest"
if ($BuildRejectedPackageForAnalysis) {
    Write-Warning "Esta variante RackForge no fue probada físicamente. No instalar."
    Write-Output "rejected_package=$candidatePackage"
    Write-Output "package_manifest=$packageManifest"
}
