[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "assets/brand/rackforge-mark.svg"
$edgeCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} "Microsoft/Edge/Application/msedge.exe"),
    (Join-Path $env:ProgramFiles "Microsoft/Edge/Application/msedge.exe"),
    (Join-Path $env:ProgramFiles "Google/Chrome/Application/chrome.exe")
)
$browser = $edgeCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $browser) {
    throw "Edge or Chrome is required to render the canonical RackForge SVG."
}
if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    throw "Python with Pillow is required to generate RackForge raster assets."
}

$render = Join-Path ([System.IO.Path]::GetTempPath()) "rackforge-brand-render-2048.png"
$uri = [System.Uri]::new((Resolve-Path -LiteralPath $source)).AbsoluteUri
try {
    $webBrand = Join-Path $repository "web/public/brand"
    New-Item -ItemType Directory -Force -Path $webBrand | Out-Null
    Copy-Item -LiteralPath (Join-Path $repository "assets/brand/rackforge-logo.svg") `
        -Destination (Join-Path $webBrand "rackforge-logo.svg") -Force
    Copy-Item -LiteralPath (Join-Path $repository "assets/brand/rackforge-mark.svg") `
        -Destination (Join-Path $webBrand "rackforge-mark.svg") -Force
    Copy-Item -LiteralPath (Join-Path $repository "assets/brand/rackforge-mark-foreground.svg") `
        -Destination (Join-Path $webBrand "rackforge-mark-foreground.svg") -Force
    Copy-Item -LiteralPath (Join-Path $repository "assets/brand/rackforge-mark.svg") `
        -Destination (Join-Path $repository "web/public/favicon.svg") -Force

    & $browser --headless=new --disable-gpu --hide-scrollbars `
        --force-device-scale-factor=1 --screenshot=$render --window-size=2048,2048 $uri | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $render)) {
        throw "The browser could not render $source."
    }

    @'
from pathlib import Path
import sys

from PIL import Image

root = Path(sys.argv[1])
source = Path(sys.argv[2])
master = Image.open(source).convert("RGBA")
resampling = Image.Resampling.LANCZOS

def save_png(path: Path, size: int, image=master):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.resize((size, size), resampling).save(path, optimize=True)

brand = root / "assets" / "brand"
web = root / "web" / "public"
android = root / "apps" / "rackforge-android" / "app" / "src" / "main" / "res"

save_png(brand / "rackforge-mark-256.png", 256)
master.save(
    brand / "rackforge.ico",
    format="ICO",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)

save_png(web / "brand" / "rackforge-mark-192.png", 192)
save_png(web / "brand" / "rackforge-mark-512.png", 512)
save_png(web / "brand" / "apple-touch-icon.png", 180)
master.save(
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
    square = master.resize((size, size), resampling)
    target = android / f"mipmap-{density}"
    target.mkdir(parents=True, exist_ok=True)
    square.save(target / "ic_launcher.png", optimize=True)

    circle_mask = Image.new("L", (size, size), 0)
    # Drawing through a resized master keeps the circle antialiased without
    # introducing an extra raster dependency.
    mask_large = Image.new("L", master.size, 0)
    from PIL import ImageDraw
    ImageDraw.Draw(mask_large).ellipse((0, 0, master.width - 1, master.height - 1), fill=255)
    circle_mask = mask_large.resize((size, size), resampling)
    round_icon = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    round_icon.paste(square, (0, 0), circle_mask)
    round_icon.save(target / "ic_launcher_round.png", optimize=True)

print("Generated RackForge brand assets from", source)
'@ | python - $repository $render
    if ($LASTEXITCODE -ne 0) {
        throw "Pillow could not generate RackForge brand assets."
    }
} finally {
    Remove-Item -LiteralPath $render -Force -ErrorAction SilentlyContinue
}
