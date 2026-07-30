# RackForge Web Plugin API

RackForge owns the web shell: global navigation, active session, security,
device pairing and host controls. A plugin owns its `PLAY` and `CONFIG`
surfaces. The host never reconstructs plugin-specific pages from assumptions.

## UI ownership

Web capabilities are not UI templates. RackForge does not prescribe a page
hierarchy, component library, visual style or editing flow for either surface.
A plugin may render any interface suitable for its own engine, use only some
host capabilities, or omit one or both web surfaces.

The generic program-editor tree is an optional interoperability mechanism. A
plugin may use it to keep hardware and web editors backed by the same typed
state, as RF-DLS does, but RackForge never renders that tree inside the
plugin's iframe. Layout, grouping and interaction remain plugin-owned.

## Package declaration

Static, pre-built assets live inside the plugin package:

```toml
[web_ui]
api_version = 1

[[web_ui.surfaces]]
kind = "play"
entry = "web/play.html"

[[web_ui.surfaces]]
kind = "config"
entry = "web/config.html"
```

Both entries are optional independently, but a declared `web_ui` must contain
at least one surface. Entry paths must be relative HTML files contained by the
package. They are validated both when the plugin package is opened and when the
web gateway indexes installed plugins.

The source framework is deliberately unspecified. React, Vue, Svelte and plain
JavaScript are all valid as long as the package contains self-contained static
HTML, CSS, JavaScript, fonts or images. Plugin views must not require a private
HTTP server.

## Trust and containment

The host renders plugin views in an iframe with
`sandbox="allow-scripts allow-same-origin"`.
Plugin HTML is served with a restrictive Content Security Policy:

- no network connections;
- no forms, media or base URL override;
- scripts, styles, fonts and images only from the installed package;
- embedding only by RackForge itself.

The iframe receives no raw Core control socket, filesystem capability or raw
session-command channel. Installed plugins are trusted packages: a native
plugin binary already has a stronger trust position than its web surface, so
the iframe and CSP are defense-in-depth containment rather than a security
boundary against a malicious installed plugin.

## Protocol v1

Messages use `window.postMessage` and always contain:

```json
{ "protocol": "rackforge.plugin.web@1" }
```

The plugin announces readiness:

```json
{
  "protocol": "rackforge.plugin.web@1",
  "kind": "ready"
}
```

RackForge responds with `kind: "context"`, the requested surface, the plugin's
own instance state, an optional matching program draft, an optional matching
audition lease and limited host state. A fresh context is sent whenever the
session revision changes.

Plugin calls are requests with a unique string `request_id`:

```json
{
  "protocol": "rackforge.plugin.web@1",
  "kind": "request",
  "request_id": "rf-dls-1",
  "method": "plugin.select_sound",
  "params": { "sound_id": "dls.b00000000.p00000000" }
}
```

The host validates the method, surface, target and value, derives the plugin
instance ID itself and returns a response. It never accepts a raw Core command
from plugin JavaScript.

## Current v1 methods

- `plugin.select_sound`: available to the plugin's own `PLAY` surface. The
  `sound_id` must appear in that instance's current catalog.
- `plugin.begin_program_edit`: available to `CONFIG`; starts a new program when
  `program_id` is `null`, or edits a program from that plugin's `custom`
  collection.
- `plugin.edit_program_field`: available to `CONFIG`; updates a field published
  by the active draft's typed editor. The host validates the draft, field and
  tagged value. `preview: true` is transient; `preview: false` confirms it.
- `plugin.set_program_name`: available to `CONFIG`; changes the active draft's
  portable program name without exposing plugin document internals.
- `plugin.restore_program_preview`: available to `CONFIG`; restores the last
  confirmed draft after transient previews.
- `plugin.save_program`: available to `CONFIG`; persists the active draft and
  releases audition focus.
- `plugin.cancel_program`: available to `CONFIG`; discards the active draft and
  releases audition focus.

These are optional capabilities, not required controls or required screens.
The host validates and transports them; the plugin decides whether and how they
appear.
