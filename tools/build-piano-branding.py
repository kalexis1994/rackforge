#!/usr/bin/env python3
"""Builds the Concert Grand's branding assets from the photograph.

docs/PLUGIN_BRANDING.md fixes the sizes and the safe areas: the icon is
512x512 with its subject inside the central 80%, the banner 1600x400 with the
left quarter kept readable because the host overlays an icon there, and the
splash 1920x1080. Nothing here bakes in text, version or state — the host
draws those.

    tools/build-piano-branding.py <source.jpg> [logo.png]
    tools/build-piano-branding.py --icon <logo.png>

The logo is the RF - Concert Grand plaque, kept at its native size in
plugins/concert-grand/artwork/logo.png; without it the icon is cropped from
the photograph's case and harp instead. The second form rebuilds the icon
alone, which is what the mark changing calls for: the photograph the banner
and splash were cut from does not live in this repository.
"""

import pathlib
import sys

from PIL import Image, ImageDraw, ImageFilter

OUT = pathlib.Path(__file__).resolve().parent.parent / "plugins/concert-grand/package/branding"
LACQUER = (0, 5, 6)
GOLD = (243, 188, 124)


def cover(image: Image.Image, size: tuple[int, int], focus: float = 0.5) -> Image.Image:
    """Scales to fill `size`, cropping the long axis around `focus` (0..1)."""
    target = size[0] / size[1]
    source = image.width / image.height
    if source > target:
        width = int(image.height * target)
        left = int((image.width - width) * focus)
        image = image.crop((left, 0, left + width, image.height))
    else:
        height = int(image.width / target)
        top = int((image.height - height) * focus)
        image = image.crop((0, top, image.width, top + height))
    return image.resize(size, Image.LANCZOS)


def contain(image: Image.Image, size: tuple[int, int]) -> Image.Image:
    """Scales to fit inside `size` whole, where `cover` crops to fill it."""
    scale = min(size[0] / image.width, size[1] / image.height)
    return image.resize(
        (round(image.width * scale), round(image.height * scale)), Image.LANCZOS
    )


def trim(logo: Image.Image) -> Image.Image:
    """Crops the transparent margin an export leaves around the mark.

    The threshold sits above zero on purpose. The plaque arrived inside a ring
    of alpha-1 pixels reaching the file's left edge — invisible on any ground,
    and enough to defeat a plain getbbox() and leave the mark swimming in the
    padding it was supposed to lose.
    """
    alpha = logo.getchannel("A").point(lambda value: 255 if value > 1 else 0)
    box = alpha.getbbox()
    return logo.crop(box) if box else logo


def vignette(image: Image.Image, strength: float = 0.55) -> Image.Image:
    """Sinks the edges into the lacquer so overlaid text stays readable."""
    mask = Image.new("L", image.size, 0)
    draw = ImageDraw.Draw(mask)
    inset_x, inset_y = image.width * 0.12, image.height * 0.12
    draw.ellipse((-inset_x, -inset_y, image.width + inset_x, image.height + inset_y), fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(min(image.size) * 0.18))
    dark = Image.new("RGB", image.size, LACQUER)
    faded = Image.blend(dark, image, 1.0 - strength)
    return Image.composite(image, faded, mask)


def banner(photo: Image.Image) -> Image.Image:
    # The piano sits centre-right in the frame; pushing the crop right keeps
    # the case and keyboard while leaving the left quarter dark for the host's
    # icon and selection number.
    image = vignette(cover(photo, (1600, 400), focus=0.58), 0.45)
    shade = Image.new("RGB", image.size, LACQUER)
    ramp = Image.new("L", image.size, 0)
    draw = ImageDraw.Draw(ramp)
    for x in range(int(image.width * 0.34)):
        draw.line([(x, 0), (x, image.height)],
                  fill=int(190 * (1 - x / (image.width * 0.34)) ** 1.5))
    return Image.composite(shade, image, ramp)


def splash(photo: Image.Image) -> Image.Image:
    return vignette(cover(photo, (1920, 1080), focus=0.5), 0.35)


def icon_from_logo(logo: Image.Image) -> Image.Image:
    # Fitted, not covered: a photograph can lose a strip off its long axis, a
    # mark cannot, and cover() would shave the plaque's own frame away. The
    # lacquer ground stays behind it because every host frames an icon as a
    # tile — rounded, shadowed, and bordered under the faceplate theme — and a
    # transparent PNG would hang in that frame over the shadow of an empty box.
    mark = contain(trim(logo.convert("RGBA")), (410, 410))   # central 80%: 410 of 512
    canvas = Image.new("RGB", (512, 512), LACQUER)
    canvas.paste(mark, ((512 - mark.width) // 2, (512 - mark.height) // 2), mark)
    return canvas


def icon_from_photo(photo: Image.Image) -> Image.Image:
    # Fall back to the case and harp: the most recognisable square of the
    # photograph at icon size.
    crop = photo.crop((int(photo.width * 0.30), int(photo.height * 0.22),
                       int(photo.width * 0.70), int(photo.height * 0.22 + photo.width * 0.40)))
    canvas = Image.new("RGB", (512, 512), LACQUER)
    canvas.paste(cover(crop, (410, 410)), (51, 51))
    return canvas


def main() -> int:
    argv = sys.argv[1:]
    icon_only = argv[:1] == ["--icon"]
    argv = argv[1:] if icon_only else argv
    if not argv or len(argv) > (1 if icon_only else 2):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    if icon_only:
        assets = {"icon.png": icon_from_logo(Image.open(argv[0]))}
    else:
        photo = Image.open(argv[0]).convert("RGB")
        assets = {"banner.png": banner(photo), "splash.png": splash(photo)}
        assets["icon.png"] = (
            icon_from_logo(Image.open(argv[1])) if len(argv) == 2 else icon_from_photo(photo)
        )
    OUT.mkdir(parents=True, exist_ok=True)
    for name, image in assets.items():
        path = OUT / name
        image.save(path, "PNG", optimize=True)
        print(f"{name}: {image.size[0]}x{image.size[1]}, {path.stat().st_size // 1024} KiB")
    return 0


raise SystemExit(main())
