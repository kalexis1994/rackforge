[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$SourceDirectory,

    [string]$HostAlias = "artupy"
)

$ErrorActionPreference = "Stop"
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$remoteDirectory = "/home/kalex/artupy/share/scva"
$expected = [ordered]@{
    "wave_1994_ver200_8mib.bin" = 8MB
    "wave_1996_rom_make_a_8mib.bin" = 8MB
    "wave_1996_rom_make_b_4mib.bin" = 4MB
    "wave_1999_sc8820_4mib.bin" = 4MB
}

$files = foreach ($entry in $expected.GetEnumerator()) {
    $path = Join-Path $source $entry.Key
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Falta el banco requerido: $path"
    }
    $item = Get-Item -LiteralPath $path
    if ($item.Length -ne $entry.Value) {
        throw (
            "Tamaño inválido para {0}: {1}; esperado {2}." -f
            $item.FullName, $item.Length, $entry.Value
        )
    }
    $item
}

& ssh -o BatchMode=yes $HostAlias (
    "install -d -m 700 '$remoteDirectory'"
)
if ($LASTEXITCODE -ne 0) {
    throw "No se pudo preparar el directorio remoto de bancos SCVA."
}

foreach ($file in $files) {
    $temporary = "${remoteDirectory}/.$($file.Name).tmp"
    & scp -o BatchMode=yes -- $file.FullName "${HostAlias}:${temporary}"
    if ($LASTEXITCODE -ne 0) {
        throw "Falló la copia de $($file.Name)."
    }
    & ssh -o BatchMode=yes $HostAlias (
        "chmod 600 '$temporary' && mv -f '$temporary' " +
        "'${remoteDirectory}/$($file.Name)'"
    )
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo instalar $($file.Name)."
    }
}

& ssh -o BatchMode=yes $HostAlias (
    "cd '$remoteDirectory' && sha256sum -- *.bin > SHA256SUMS && " +
    "chmod 600 SHA256SUMS && sha256sum --check SHA256SUMS"
)
if ($LASTEXITCODE -ne 0) {
    throw "Falló la verificación remota de los bancos SCVA."
}

Write-Host "Bancos SCVA instalados fuera de Git en $remoteDirectory."
