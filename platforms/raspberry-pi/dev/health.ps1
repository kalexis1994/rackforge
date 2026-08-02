[CmdletBinding()]
param(
    [string]$HostAlias = "rackforge"
)

$ErrorActionPreference = "Stop"

$script = @'
set -eu
printf 'host='
hostname
printf 'kernel='
uname -r
printf 'uptime='
cut -d. -f1 /proc/uptime
free -h
df -h /
if command -v vcgencmd >/dev/null; then
    vcgencmd measure_temp
    vcgencmd get_throttled
fi
printf '%s\n' '--- USB ---'
lsusb
printf '%s\n' '--- ALSA ---'
aplay -l
arecord -l
if [ -r "$HOME/rackforge/current/platforms/raspberry-pi/audio/probe.sh" ]; then
    printf '%s\n' '--- RACKFORGE AUDIO ---'
    bash "$HOME/rackforge/current/platforms/raspberry-pi/audio/probe.sh"
fi
'@
$payload = [Convert]::ToBase64String(
    [Text.Encoding]::UTF8.GetBytes($script.Replace("`r", ""))
)

& ssh -o BatchMode=yes $HostAlias (
    "printf '%s' '$payload' | base64 --decode | bash"
)
if ($LASTEXITCODE -ne 0) {
    throw "El diagnóstico remoto terminó con código $LASTEXITCODE."
}
