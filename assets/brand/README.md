# RackForge brand assets

`rackforge-logo.svg` is the canonical RackForge logo. Keep its geometry and
wordmark outlines unchanged.

## Palette

| Role | Value | Use |
| --- | --- | --- |
| Forge black | `#050B11` | Primary logo and launcher background |
| Signal cyan | `#55E7FF` | Mark, wordmark and active brand accents |
| Ice highlight | `#D7FBFF` | Small highlights only; never replace the core cyan |

## Variants

- `rackforge-logo.svg`: full lockup for splash screens, About views and artwork
  at least 160 px wide.
- `rackforge-mark.svg`: optically corrected compact mark on Forge black for app
  icons, favicons and small square surfaces.
- `rackforge-mark-foreground.svg`: transparent compact mark for dark in-app
  surfaces and adaptive-icon foregrounds.
- `rackforge.ico`: multi-resolution Windows executable icon.
- `rackforge-mark-256.png`: runtime window icon used by the desktop host.

The compact mark preserves the logo path but uses a heavier stroke so the
signal route remains visible at 16–48 px. Keep at least 12% clear space around
it. Do not use the full wordmark below 160 px because it becomes unreadable.

Android uses the compact mark inside the platform's 66/108 adaptive-icon safe
zone. The operating system supplies the final circle, squircle or rounded
square mask.
