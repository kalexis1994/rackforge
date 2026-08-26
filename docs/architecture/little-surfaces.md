# LITTLE responsive surface architecture

LITTLE is RackForge's compact, host-owned controller UI. It is an interaction
contract, not an 18-character Arturia screen and not a plugin-provided Web
view.

## Ownership

RackForge owns navigation, presets, program selection, loading states, errors,
and the active session. A `.rfcontroller` owns hardware discovery, input
gestures, display transport, LEDs, and projection onto the physical screen.
Plugins publish data only:

- a full `name` and compact `short_name`;
- at least one native PROGRAM;
- optional program editor pages and fields;
- a public parameter schema grouped into pages.

A plugin does not need to declare `little@1` to appear in PLAY. RackForge can
render the plugin identity, host presets, and native program catalog itself.

## One semantic contract, responsive projection

`little@1` describes the information hierarchy and the Previous, Next,
Confirm, and Back actions. It does not describe a screen width. Each controller
implementation declares a `SurfaceViewport` independently:

```toml
[[surfaces]]
layout_id = "little@1"
quality = "native"
priority = 0

[surfaces.viewport]
text_columns = 18
header_rows = 1
body_rows = 2
soft_keys = 4
```

The surface runtime emits semantic header fields (`plugin name`, `short name`,
and `context`). The controller projection uses the complete name when it fits,
falls back to `short_name` when constrained, and divides the available width
between identity and context. The Arturia viewport naturally produces an
approximately 8 + 1 + 9 arrangement. A wider screen receives more information;
a narrower one truncates safely. No `medium@1` or `wide@1` breakpoint is needed.

A different layout ID is appropriate only when navigation meaning changes—for
example, a touch grid that exposes several selectable rows simultaneously—not
because the display gained pixels or columns.

## Plugin navigation

Inside a standalone PLAY plugin, the stable host-owned order is:

1. PRESETS — portable `.rfpreset` snapshots owned by RackForge.
2. PROGRAMS — the plugin's native factory/user program catalog.
3. CONTROLS — every public parameter grouped by the plugin's declarative pages.

CONTROLS uses the ordinary host parameter path. Number, boolean and enum
changes therefore reach the same validated `ParameterEvent` flow as the Web
panel and MIDI Links; triggers emit an explicit press/release pair. Plugins do
not implement controller-specific menus. They only publish names, order,
domains, choices and read-only state in their parameter schema.

Parameter schema v3 may also publish a plugin-wide `display_decimals` value
from zero through six. LITTLE uses it only to format floating-point values;
the parameter `step` remains the editing and runtime resolution. When omitted,
RackForge derives precision from `step` for compatibility with older plugins.

Rack Slots intentionally start from PROGRAMS until isolated Rack-preset
attachment is supported. Loading a host preset or native program always uses
the normal host control path; a controller never mutates plugin memory.

## Compatibility

Manifest schema 3 requires an ASCII `short_name` of one to eight characters.
Schema 1 and 2 packages remain valid and receive a deterministic compact name
derived from `name`. Session snapshots also default a missing compact name so
older hosts and persisted sessions remain readable.

The current body renderer provides the canonical two-line representation. New
controllers may enrich that representation from the same semantic model, but
must preserve navigation and command behavior. Geometry-specific behavior
belongs to the `.rfcontroller`, never to an `.rfplugin`.
