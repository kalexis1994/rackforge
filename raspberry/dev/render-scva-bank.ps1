[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$SCCorePath,

    [Parameter(Mandatory)]
    [string]$OutputDirectory,

    [ValidateRange(0, 127)]
    [int]$Program = 0,

    [ValidateRange(1, 127)]
    [int]$Velocity = 100,

    [ValidateRange(0, 127)]
    [int]$FirstNote = 36,

    [ValidateRange(0, 127)]
    [int]$LastNote = 96,

    [switch]$Resume
)

$ErrorActionPreference = "Stop"
if ($FirstNote -gt $LastNote) {
    throw "FirstNote must not exceed LastNote."
}

$dll = (Resolve-Path -LiteralPath $SCCorePath).Path
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..")).Path
$manifest = Join-Path $root "raspberry\engines\nuked-sc55\tools\scva-render\Cargo.toml"
$knownMingw = "C:\msys64\ucrt64\bin"
if (-not (Get-Command dlltool.exe -ErrorAction SilentlyContinue) -and
    (Test-Path -LiteralPath (Join-Path $knownMingw "dlltool.exe"))) {
    $env:Path = "$knownMingw;$env:Path"
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path

& cargo build --release --manifest-path $manifest --bin scva-render
if ($LASTEXITCODE -ne 0) {
    throw "scva-render did not build successfully."
}
$renderer = Join-Path (Split-Path -Parent $manifest) "target\release\scva-render.exe"

foreach ($note in $FirstNote..$LastNote) {
    $path = Join-Path $output ("note-{0:D3}.wav" -f $note)
    if (Test-Path -LiteralPath $path) {
        if ($Resume) {
            Write-Host "SKIP note=$note output=$path"
            continue
        }
        throw "Output already exists: $path. Use -Resume to keep completed notes."
    }
    & $renderer --program $Program --note $note --velocity $Velocity $dll $path
    if ($LASTEXITCODE -ne 0) {
        throw "SCCore rendering failed at MIDI note $note."
    }
}

$metadata = [ordered]@{
    format = "rackforge-rendered-bank-v1"
    source = [IO.Path]::GetFileName($dll)
    program = $Program
    velocity = $Velocity
    first_note = $FirstNote
    last_note = $LastNote
    generated_utc = [DateTime]::UtcNow.ToString("O")
}
$metadata | ConvertTo-Json |
    Set-Content -LiteralPath (Join-Path $output "bank.json") -Encoding utf8
Write-Host "RENDERED_BANK_READY directory=$output notes=$($LastNote - $FirstNote + 1)"
