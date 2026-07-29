[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$SourceDirectory,

    [ValidateSet("mk1", "mk2")]
    [string]$Model = "mk2",

    [string]$HostAlias = "artupy"
)

$ErrorActionPreference = "Stop"
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$remoteDirectory = "/home/kalex/artupy/share/nuked-sc55"

$sets = @{
    mk2 = @(
        "rom1.bin",
        "rom2.bin",
        "rom_sm.bin",
        "waverom1.bin",
        "waverom2.bin"
    )
    mk1 = @(
        "sc55_rom1.bin",
        "sc55_rom2.bin",
        "sc55_waverom1.bin",
        "sc55_waverom2.bin",
        "sc55_waverom3.bin"
    )
}

$files = foreach ($name in $sets[$Model]) {
    $path = Join-Path $source $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Falta el archivo requerido: $path"
    }
    $item = Get-Item -LiteralPath $path
    if ($item.Length -eq 0) {
        throw "El archivo está vacío: $path"
    }
    $item
}

& ssh -o BatchMode=yes $HostAlias (
    "install -d -m 700 '$remoteDirectory'"
)
if ($LASTEXITCODE -ne 0) {
    throw "No se pudo preparar el directorio remoto de ROMs."
}

foreach ($file in $files) {
    & scp -o BatchMode=yes -- $file.FullName (
        "${HostAlias}:${remoteDirectory}/$($file.Name)"
    )
    if ($LASTEXITCODE -ne 0) {
        throw "Falló la copia de $($file.Name)."
    }
}

& ssh -o BatchMode=yes $HostAlias (
    "chmod 600 '$remoteDirectory'/*.bin && " +
    "cd '$remoteDirectory' && sha256sum -- *.bin > SHA256SUMS && " +
    "cat SHA256SUMS"
)
if ($LASTEXITCODE -ne 0) {
    throw "No se pudieron verificar las ROMs copiadas."
}

Write-Host "ROM set $Model instalado fuera de Git."
