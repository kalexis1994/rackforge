[CmdletBinding()]
param()

# Regenerates every raster brand asset from the SVGs in assets/brand, and
# copies the vector originals into web/public where the interface loads them.
#
# Two masters are rendered rather than one: the launcher plate is full bleed
# because the platform may crop it to any shape, and the favicon is a rounded
# plate with the mark set wider, because a browser tab has no safe zone to
# respect and shows the artwork at 16-32px.

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$brandSource = Join-Path $repository "assets/brand"
$iconSource = Join-Path $brandSource "rackforge-icon.svg"
$faviconSource = Join-Path $brandSource "favicon.svg"

$edgeCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} "Microsoft/Edge/Application/msedge.exe"),
    (Join-Path $env:ProgramFiles "Microsoft/Edge/Application/msedge.exe"),
    (Join-Path $env:ProgramFiles "Google/Chrome/Application/chrome.exe")
)
$browser = $edgeCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $browser) {
    throw "Edge or Chrome is required to render the canonical RackForge SVGs."
}
if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    throw "Python with Pillow is required to generate RackForge raster assets."
}

function Invoke-SvgRender {
    param(
        [Parameter(Mandatory)] [string] $Source,
        [Parameter(Mandatory)] [string] $Destination,
        [Parameter(Mandatory)] [int] $Size
    )

    $uri = [System.Uri]::new((Resolve-Path -LiteralPath $Source)).AbsoluteUri
    # A throwaway profile per render: a second headless run against the user's
    # own profile attaches to the running browser and exits without drawing.
    $profileDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rackforge-brand-" + [guid]::NewGuid())
    try {
        # The size pair has to reach the browser as one argument: unquoted, the
        # comma makes PowerShell split it into two.
        & $browser --headless=new --disable-gpu --hide-scrollbars `
            --force-device-scale-factor=1 "--user-data-dir=$profileDir" `
            "--screenshot=$Destination" "--window-size=$Size,$Size" $uri | Out-Null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Destination)) {
            throw "The browser could not render $Source."
        }
    } finally {
        Remove-Item -LiteralPath $profileDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$temp = [System.IO.Path]::GetTempPath()
$iconRender = Join-Path $temp "rackforge-icon-2048.png"
$faviconRender = Join-Path $temp "rackforge-favicon-512.png"

try {
    $webPublic = Join-Path $repository "web/public"
    $webBrand = Join-Path $webPublic "brand"
    New-Item -ItemType Directory -Force -Path $webBrand | Out-Null

    foreach ($vector in @(
        "rackforge-logo.svg",
        "rackforge-mark.svg",
        "rackforge-icon.svg",
        "rackforge-mark-foreground.svg"
    )) {
        Copy-Item -LiteralPath (Join-Path $brandSource $vector) `
            -Destination (Join-Path $webBrand $vector) -Force
    }
    Copy-Item -LiteralPath $faviconSource `
        -Destination (Join-Path $webPublic "favicon.svg") -Force

    Invoke-SvgRender -Source $iconSource -Destination $iconRender -Size 2048
    Invoke-SvgRender -Source $faviconSource -Destination $faviconRender -Size 512

    @'
from pathlib import Path
import sys

from PIL import Image, ImageDraw

root = Path(sys.argv[1])
icon_master = Image.open(sys.argv[2]).convert("RGBA")
favicon_master = Image.open(sys.argv[3]).convert("RGBA")
resampling = Image.Resampling.LANCZOS

def save_png(path: Path, size: int, image=icon_master):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.resize((size, size), resampling).save(path, optimize=True)

brand = root / "assets" / "brand"
web = root / "web" / "public"
android = root / "apps" / "rackforge-android" / "app" / "src" / "main" / "res"

save_png(brand / "rackforge-mark-256.png", 256)
icon_master.save(
    brand / "rackforge.ico",
    format="ICO",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)

save_png(web / "brand" / "rackforge-mark-192.png", 192)
save_png(web / "brand" / "rackforge-mark-512.png", 512)
save_png(web / "brand" / "apple-touch-icon.png", 180)

# The tab icon keeps its own master: rounded plate, mark set wider.
favicon_master.save(
    web / "favicon.ico",
    format="ICO",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48)],
)

densities = {
    "mdpi": 48,
    "hdpi": 72,
    "xhdpi": 96,
    "xxhdpi": 144,
    "xxxhdpi": 192,
}
for density, size in densities.items():
    square = icon_master.resize((size, size), resampling)
    target = android / f"mipmap-{density}"
    target.mkdir(parents=True, exist_ok=True)
    square.save(target / "ic_launcher.png", optimize=True)

    # Drawing through a resized master keeps the circle antialiased without
    # introducing an extra raster dependency.
    mask_large = Image.new("L", icon_master.size, 0)
    ImageDraw.Draw(mask_large).ellipse(
        (0, 0, icon_master.width - 1, icon_master.height - 1), fill=255
    )
    circle_mask = mask_large.resize((size, size), resampling)
    round_icon = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    round_icon.paste(square, (0, 0), circle_mask)
    round_icon.save(target / "ic_launcher_round.png", optimize=True)

print("Generated RackForge brand assets")
'@ | python - $repository $iconRender $faviconRender
    if ($LASTEXITCODE -ne 0) {
        throw "Pillow could not generate RackForge brand assets."
    }
} finally {
    Remove-Item -LiteralPath $iconRender -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $faviconRender -Force -ErrorAction SilentlyContinue
}
