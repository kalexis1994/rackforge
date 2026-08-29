# RackForge brand assets

`rackforge-mark.svg` is the canonical artwork: an RF monogram drawn as a signal
path, from the input jack on the left to the output arrow on the right. Every
other file here is the same geometry in a different frame.

Run `tools/generate-brand-logo.py` after changing the mark or the interface's
display typeface, then `tools/generate-brand-assets.ps1` after changing any SVG
in this directory. The first draws the nameplate, the second re-renders every
raster and copies the vectors into `web/public`; editing generated files
directly is wasted work, because the next run overwrites them.

## The paint order is the drawing

The mark is four strokes and three fills, and their order is load-bearing:

1. the leg and the foot of the F,
2. the top arm of the F,
3. the crossbar,
4. the bowl of the R — **over** the leg, so the bar truncates the leg at its
   lower edge instead of letting the diagonal tip run up into the red,
5. the jack, then the node at the crossbar, which covers the seam where three
   strokes meet, then the output arrow.

Reordering them is a visual change, not a tidy-up. The two nodes are knockouts
punched through everything beneath, so they are drawn with a mask; Android's
vector drawables have no masks, and rebuild the same holes by stopping each
stroke short of the node and filling an even-odd ring.

## Palette

One ink per element, and a second set mixed for a dark ground. The interface
carries the same values as `--mark-bowl`, `--mark-leg` and `--mark-arm`.

| Role | Daylight | Stage | Use |
| --- | --- | --- | --- |
| Plate | `#EBE4D8` | `#212327` | The surface the mark is printed on |
| Bowl | `#C1273D` | `#E0455C` | The D of the R, and the jack its line starts from |
| Leg | `#1D3560` | `#4A79C8` | The slanted leg and the foot of the F |
| Arm | `#D25A2B` | `#F0813F` | The whole F: top arm, crossbar, node, arrow |

Do not recolour a single element on its own. The split reads as one ink per
piece of the monogram; three arbitrary colours read as a rainbow.

## Variants

- `rackforge-mark.svg`: the mark, trimmed to the ink (704×308). Use it on a
  surface that already provides its own ground.
- `rackforge-logo.svg`: the mark on a plate with its clear space measured in —
  148 units at the sides, 76 above and below. Use where the artwork needs a
  frame of its own.
- `rackforge-lockup.svg`: the nameplate — mark over the word, flush left, on the
  same plate. **Generated** by `tools/generate-brand-logo.py`, which outlines
  RACKFORGE from the Barlow Semi Condensed the interface itself loads, at the
  proportion the rail sets it: 13px of type under a 140px mark, 9px apart. This
  is the logo to hand to anything outside the app — a README, a store listing,
  a slide. Inside the app the rail composes the same lockup live, from the
  inlined mark and real text.

  It carries the plate rather than sitting transparent, and that is not
  decoration. The mark survives any ground because its three inks are
  mid-tone; the word is a single ink, so on a light card while the system is
  in dark mode it renders cream on cream. Ink and ground have to travel
  together. Artwork that must sit on a surface you already control is
  `rackforge-mark.svg`.
- `rackforge-icon.svg`: the launcher plate. Full bleed, because the platform may
  crop it to any shape, with the mark held to 600 units wide so it stays inside
  the 66/108 adaptive-icon safe circle. This is the master for every raster.
- `favicon.svg`: rounded plate with the mark set wider, because a browser tab
  has no safe zone and renders at 16–32 px. Its own raster master.
- `rackforge-mark-foreground.svg`: transparent square. The boot loader stacks
  two copies and reveals one from the bottom, and adaptive-icon foregrounds
  expect the plate to come from elsewhere.
- `rackforge.ico`: multi-resolution Windows executable icon.
- `rackforge-mark-256.png`: runtime window icon used by the desktop host.

The vector files follow the system light through `prefers-color-scheme`; the
launcher plate is flat daylight ink, since it is only ever rasterised.

Keep at least 12% clear space around the mark. Below roughly 32 px the node
knockouts close up — that is expected, and it is why the launcher plate is
generated rather than the mark being redrawn for small sizes.
