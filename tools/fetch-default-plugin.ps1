[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Output
)

$ErrorActionPreference = 'Stop'
$assetName = 'RF-Soundfonts.rfplugin'
$releaseBase = 'https://github.com/kalexis1994/rackforge-plugin-rf-soundfonts/releases/latest/download'
$outputPath = [System.IO.Path]::GetFullPath($Output)
$outputDirectory = Split-Path -Parent $outputPath
$temporary = Join-Path ([System.IO.Path]::GetTempPath()) `
    ('rackforge-default-plugin-' + [guid]::NewGuid().ToString('N'))
$archive = Join-Path $temporary $assetName
$checksumFile = "$archive.sha256"

New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    Invoke-WebRequest -Uri "$releaseBase/$assetName" -OutFile $archive `
        -MaximumRetryCount 3 -RetryIntervalSec 2
    Invoke-WebRequest -Uri "$releaseBase/$assetName.sha256" -OutFile $checksumFile `
        -MaximumRetryCount 3 -RetryIntervalSec 2

    $checksumText = (Get-Content -Raw -LiteralPath $checksumFile).Trim()
    $pattern = '^([0-9a-fA-F]{64})\s+RF-Soundfonts\.rfplugin$'
    if ($checksumText -notmatch $pattern) {
        throw 'The RF-Soundfonts release checksum has an invalid format.'
    }
    $expected = $Matches[1].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "RF-Soundfonts checksum mismatch: expected $expected, got $actual."
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $entries = @($zip.Entries | ForEach-Object FullName)
        foreach ($required in @(
            'rackforge-plugin.toml',
            'component.wasm',
            'LICENSE',
            'assets/ydp-grand-piano.sf2',
            'metadata/runtime.json',
            'metadata/presets.json'
        )) {
            if ($required -notin $entries) {
                throw "RF-Soundfonts package is missing $required."
            }
        }
        $manifestEntry = $zip.GetEntry('rackforge-plugin.toml')
        $reader = [System.IO.StreamReader]::new($manifestEntry.Open())
        try {
            $manifest = $reader.ReadToEnd()
        } finally {
            $reader.Dispose()
        }
        if ($manifest -notmatch '(?m)^id = "org\.rackforge\.rf-soundfonts"\s*$') {
            throw 'The default plugin release has an unexpected package identity.'
        }
        if ($manifest -notmatch '(?m)^version = "([^"]+)"\s*$') {
            throw 'The default plugin release does not declare a version.'
        }
        $version = $Matches[1]

        $runtimeEntry = $zip.GetEntry('metadata/runtime.json')
        $reader = [System.IO.StreamReader]::new($runtimeEntry.Open())
        try {
            $runtime = $reader.ReadToEnd() | ConvertFrom-Json
        } finally {
            $reader.Dispose()
        }
        if ($runtime.id -ne 'org.rackforge.rf-soundfonts' -or
            $runtime.version -ne $version) {
            throw 'The default plugin runtime metadata does not match its manifest.'
        }
    } finally {
        $zip.Dispose()
    }

    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    $staged = "$outputPath.new"
    Copy-Item -LiteralPath $archive -Destination $staged -Force
    Move-Item -LiteralPath $staged -Destination $outputPath -Force
    Write-Output "DEFAULT_PLUGIN_READY path=$outputPath version=$version sha256=$actual"
} finally {
    $resolved = [System.IO.Path]::GetFullPath($temporary)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolved) -like 'rackforge-default-plugin-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
