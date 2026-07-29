[CmdletBinding()]
param(
    [ValidateRange(1, 300)]
    [int] $Seconds = 60
)

$ErrorActionPreference = 'Stop'
$deadline = (Get-Date).AddSeconds($Seconds)
$lastSignature = ''

Write-Host 'Read-only monitor. No USB command will be sent.'
Write-Host 'Normal mode is service arturiausbmidi. Bootloader should differ.'

do {
    $devices = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object {
            $_.InstanceId -match 'VID_1C75&PID_028C' -or
            $_.FriendlyName -match 'Arturia.*(Boot|Firmware)|DFU|Firmware Updater'
        }

    $rows = foreach ($device in $devices) {
        $service = (
            Get-PnpDeviceProperty `
                -InstanceId $device.InstanceId `
                -KeyName 'DEVPKEY_Device_Service' `
                -ErrorAction SilentlyContinue
        ).Data

        [PSCustomObject]@{
            Status     = $device.Status
            Class      = $device.Class
            Name       = $device.FriendlyName
            Service    = $service
            InstanceId = $device.InstanceId
        }
    }

    $signature = $rows | ConvertTo-Json -Compress
    if ($signature -ne $lastSignature) {
        Write-Host (Get-Date -Format 'HH:mm:ss')
        if ($rows) {
            $rows | Format-Table -AutoSize
        } else {
            Write-Host 'KeyLab not present'
        }
        $lastSignature = $signature
    }

    $bootCandidate = $rows | Where-Object {
        $_.InstanceId -match '^USB\\VID_1C75&PID_028C' -and
        $_.Service -ne 'arturiausbmidi'
    }
    if ($bootCandidate) {
        Write-Host 'Bootloader/non-MIDI USB interface detected.'
        $bootCandidate | Format-List
        exit 0
    }

    Start-Sleep -Milliseconds 500
} while ((Get-Date) -lt $deadline)

Write-Host 'Timeout: device remained in normal MIDI mode.'
exit 1
