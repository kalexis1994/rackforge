[CmdletBinding()]
param(
    [string]$HostAlias = "rackforge",
    [string]$RemoteRoot = "rackforge"
)

$ErrorActionPreference = "Stop"
$source = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..\..")).Path
$archive = Join-Path ([System.IO.Path]::GetTempPath()) (
    "rackforge-raspberry-{0}.tar" -f [guid]::NewGuid().ToString("N")
)
$fileList = $null

try {
    $tracked = @(& git -C $source ls-files)
    if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) {
        throw "No se pudo enumerar el contenido versionado."
    }
    $deploymentFiles = @(
        $tracked | Where-Object {
            $_ -ne "config/repositories.toml" -and
            (Test-Path -LiteralPath (Join-Path $source $_))
        }
    )
    $fileList = Join-Path ([System.IO.Path]::GetTempPath()) (
        "rackforge-raspberry-files-{0}.txt" -f [guid]::NewGuid().ToString("N")
    )
    [IO.File]::WriteAllLines($fileList, $deploymentFiles, [Text.UTF8Encoding]::new($false))
    & tar -C $source -cf $archive -T $fileList
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo crear el paquete de despliegue."
    }

    & scp $archive "${HostAlias}:/tmp/rackforge-raspberry.tar"
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo copiar el paquete a la Raspberry."
    }

$remote = @"
set -eu
install -d '$RemoteRoot/current' '$RemoteRoot/banks' \
  '$RemoteRoot/bin' '$RemoteRoot/build' '$RemoteRoot/performances' \
  '$RemoteRoot/share/nuked-sc55' '$RemoteRoot/state' '$RemoteRoot/logs'
tar -xf /tmp/rackforge-raspberry.tar -C '$RemoteRoot/current'
rm -f /tmp/rackforge-raspberry.tar
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
    if ($fileList -and (Test-Path -LiteralPath $fileList)) {
        Remove-Item -LiteralPath $fileList -Force
    }
    if (Test-Path -LiteralPath $archive) {
        Remove-Item -LiteralPath $archive -Force
    }
}
