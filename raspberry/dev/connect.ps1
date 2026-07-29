[CmdletBinding()]
param(
    [string]$HostAlias = "artupy"
)

$ErrorActionPreference = "Stop"
& ssh $HostAlias
if ($LASTEXITCODE -ne 0) {
    throw "La conexión SSH terminó con código $LASTEXITCODE."
}
