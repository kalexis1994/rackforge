[CmdletBinding()]
param(
    [string]$HostAlias = "artupy",
    [string]$RemoteRoot = "/home/kalex/artupy"
)

$ErrorActionPreference = "Stop"
$source = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$archive = Join-Path ([System.IO.Path]::GetTempPath()) (
    "artupy-raspberry-{0}.tar" -f [guid]::NewGuid().ToString("N")
)

try {
    & tar -C $source --exclude=target --exclude=.git -cf $archive .
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo crear el paquete de despliegue."
    }

    & scp $archive "${HostAlias}:/tmp/artupy-raspberry.tar"
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo copiar el paquete a la Raspberry."
    }

$remote = @"
set -eu
install -d '$RemoteRoot/current' '$RemoteRoot/banks' \
  '$RemoteRoot/performances' '$RemoteRoot/state' '$RemoteRoot/logs'
tar -xf /tmp/artupy-raspberry.tar -C '$RemoteRoot/current'
rm -f /tmp/artupy-raspberry.tar
printf 'deployed=%s\n' '$RemoteRoot/current'
"@
    $payload = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($remote.Replace("`r", ""))
    )

    & ssh -o BatchMode=yes $HostAlias (
        "printf '%s' '$payload' | base64 --decode | bash"
    )
    if ($LASTEXITCODE -ne 0) {
        throw "El despliegue remoto no terminó correctamente."
    }
}
finally {
    if (Test-Path -LiteralPath $archive) {
        Remove-Item -LiteralPath $archive -Force
    }
}
