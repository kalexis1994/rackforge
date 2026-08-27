# Plugin branding

RackForge plugins provide one visual identity that is rendered consistently by
Desktop, Android, Raspberry Pi, and remote Web clients. These images belong to
the immutable `.rfplugin` package. They are display-only: no executable SVG,
HTML, external URL, platform icon bundle, or filesystem path crosses into the
application shell.

## Required assets

| Asset | Format and dimensions | Maximum size | Host use |
| --- | --- | ---: | --- |
| Icon | 512×512 static 8-bit RGB/RGBA PNG | 2 MiB | Compact cards, bars, and plugin identity |
| Banner | 1600×400 static 8-bit RGB/RGBA PNG | 4 MiB | PLAY selector and wide catalog rows |
| Splash | 1920×1080 static 8-bit RGB/RGBA PNG | 8 MiB | Loading state and large-screen preview |

Use PNG source files, not `.ico`. A Windows `.ico` is an application-launcher
container and is not a portable UI asset. RackForge chooses the rendered size
and downscales the canonical PNG where needed.

Place important content inside these safe areas:

- Icon: central 80% on both axes. Leave corners free for masks and badges.
- Banner: central 80% horizontally and 70% vertically. Keep the left 25%
  readable because RackForge may overlay an icon and selection number.
- Splash: central 80% on both axes. The host may crop the outer region on wide
  or tall screens.

Do not bake version numbers, state, buttons, or loading messages into the
images. RackForge overlays live information and localizes host controls.
The banner is also shown in the package preview before the user confirms an
installation.

## Manifest contract

Branding is mandatory in manifest schema 2 and newer (new packages use schema
3, which also requires `short_name`):

```toml
schema_version = 3
short_name = "RF-EX"

[branding]
icon = "branding/icon.png"
banner = "branding/banner.png"
splash = "branding/splash.png"
background_color = "#07131C"
accent_color = "#55E7FF"
```

The colors are optional `#RRGGBB` hints. `background_color` fills uncovered
space around the splash or plugin surface. `accent_color` may tint selection
borders and status details. RackForge preserves accessible text contrast and
does not allow either hint to replace semantic host colors.

Paths must be relative, remain inside the package, and end in lowercase
`.png`. During installation and again before activation, RackForge resolves
each file against the canonical package directory, rejects missing or escaped
files, fully decodes the PNG, and verifies dimensions, encoding, animation,
and byte limits.

## Compatibility

Manifest schema 1 packages remain supported and receive the generic RackForge
plugin identity. They cannot declare a partial branding section. This makes the
fallback predictable and lets a plugin move atomically to schema 2 once all
three production assets are ready.

The public plugin descriptor contains host-owned `icon_url`, `banner_url`, and
`splash_url` values. A UI must consume those URLs rather than constructing or
reading package paths. The same `/plugin-assets/<plugin-id>/...` boundary is
implemented by Desktop, Android, and the Raspberry Pi Web host.

The pure Browser host has no package HTTP server. It publishes dynamically
installed package files into a service-worker-owned Cache Storage namespace.
Package lifecycle responses are linked to a specific storage snapshot and do
not complete until that snapshot's HTML, JavaScript, CSS, Wasm, and branding
files have crossed the publication barrier. Catalog descriptors expose no
dynamic URL before that point. Reinstalls advance an asset generation so an
iframe that observed an earlier cache miss is remounted deterministically;
uninstall removes every cache entry absent from the authoritative snapshot.
Bundled plugins continue to use immutable files below `demo/rackforge/plugins/`
and do not depend on this worker-backed route.
