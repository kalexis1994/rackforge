# RackForge Web Plugin API

RackForge owns the web shell: global navigation, active session, security,
device access and host controls. A plugin owns its `PLAY` and `CONFIG`
surfaces. The host never reconstructs plugin-specific pages from assumptions.

## UI ownership

Web capabilities are not UI templates. RackForge does not prescribe a page
hierarchy, component library, visual style or editing flow for either surface.
A plugin may render any interface suitable for its own engine, use only some
host capabilities, or omit one or both web surfaces.

`PLAY` may be the plugin's complete instrument editor, matching the
single-window model used by many native audio plugins. `CONFIG` is optional and
should be declared only for genuinely separate, infrequent setup such as sound
libraries, resources, compatibility options or diagnostics. Program selection,
sound design and Custom Program editing belong to `PLAY`. The manifest's
`config_mode` flag declares the separate settings capability to controller
surfaces such as LITTLE. Hosts keep CONFIG discoverable and show an explicit
unavailable message when the flag or corresponding web surface is absent.

RackForge presents the two surfaces in different places rather than exposing a
PLAY/CONFIG tab switch. PLAY is the performance workspace. Opening a running
plugin from the Plugins section loads its CONFIG surface directly.

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

- `plugin.parameters`: available to `PLAY` and `CONFIG`; returns the active
  instance's validated parameter schema and current finite values. Plugins use
  this instead of assuming their factory defaults still match runtime state.
- `plugin.set_parameter`: available to `PLAY` and `CONFIG`; accepts one public parameter
  index and finite value. RackForge verifies ownership, writability, type and
  range before the audio thread calls the plugin ABI.
- `plugin.select_sound`: available to the plugin's own `PLAY` and `CONFIG`
  surfaces. The
  `sound_id` must appear in that instance's current catalog.
- `plugin.set_surface_info`: available to `PLAY`; publishes optional ephemeral
  text in the center slot of RackForge's compact performance bar. `label` is
  limited to 24 characters and `value` to 96. Sending an empty or null `value`
  clears the slot. The host owns presentation, escapes all text and clears the
  value when the surface or active instance changes. When no value is
  published, RackForge shows the instance's currently selected program.
- `plugin.select_resource`: available to `CONFIG`; accepts only a `resource_id`
  declared by that plugin's manifest. RackForge opens its host-owned resource
  explorer and returns a grant with an opaque `grant_id`, display name and
  selected kind. The plugin cannot supply its own plugin ID, browse native
  paths or receive a Windows path, POSIX path or Android content URI.
- `plugin.resource_bindings`: available to `PLAY` and `CONFIG`; returns the
  persistent opaque grants owned by the current plugin. It takes no parameters.
  `PLAY` can use this read-only collection to present libraries that were
  previously added in `CONFIG`; it cannot open a new host resource selector.
- `plugin.resource_status`: available to `CONFIG`; reports whether each
  declared private `data_path` currently contains an installed regular file.
  It returns resource identifiers and booleans without exposing native paths.
- `plugin.resource_entries`: available to `CONFIG`; lists one level inside an
  existing grant. It accepts `grant_id` and an optional `parent_id`, both
  opaque. Passing no `parent_id` lists the grant root.
- `plugin.load_resource`: available to `CONFIG`; loads one granted file into a
  file resource declared by the active plugin. It accepts
  `target_resource_id`, `grant_id`, `entry_id` and an optional `bundle`.
  `nki_dependencies` packages the selected NKI with nearby samples and artwork;
  `sfz_dependencies` packages the selected SFZ with its include documents,
  samples and artwork. RackForge prepares a new
  plugin instance away from the real-time audio callback and swaps it at an
  audio block boundary.
- `plugin.preview_resource`: available to `CONFIG`; accepts a declared file
  resource target, `edit_mode: 1`, a display file name and a transferable
  `ArrayBuffer` up to 128 MB. RackForge rejects preview requests outside this
  explicit editing state (`edit_mode: 0`). It validates the resource in a
  replacement instance and swaps that instance at an audio block boundary
  without installing the file or changing the persistent resource registry.
  The preview resource selects its first playable sound, which lets an editor
  audition an in-progress document.
- `plugin.install_resource`: available to `CONFIG`; asks a fresh plugin
  instance to accept a granted file and then copies it atomically to the
  resource's declared private `data_path`. File grants omit `entry_id`;
  directory grants provide the selected child `entry_id`. RackForge activates
  a replacement instance when the installed set is already complete; partial
  sets remain safely installed for the next import. Future instances resolve
  installed resources automatically. When the selected manifest resource has
  `import_targets`, the granted ZIP is only an import container: RackForge
  authenticates its entries with the plugin, persists each recognized target
  separately, and returns `installed_resource_ids`. Repeated ZIP imports are
  cumulative; the ZIP itself is not reported as installed.
- `plugin.activate_resource`: available to `PLAY`; activates and persists one
  file grant that the same plugin previously added through `CONFIG`. It accepts
  the same target, grant and optional bundle fields as `plugin.install_resource`,
  but cannot create a grant or browse storage. This keeps library management in
  `CONFIG` while allowing musical library selection from `PLAY`.
- `plugin.clear_resource`: available to `CONFIG`; removes one installed private
  resource. It accepts `target_resource_id`, which must be a declared file
  resource with a `data_path`. RackForge deletes the installed file, prepares a
  replacement instance from the remaining resource set away from the real-time
  audio callback, and swaps it at an audio block boundary, so the package
  default is playing again. Required resources cannot be cleared.
- `plugin.begin_program_edit`: available to `PLAY` and `CONFIG`; starts a new
  program when `program_id` is `null`, or edits a program from that plugin's
  `custom` collection.
- `plugin.edit_program_field`: available to `PLAY` and `CONFIG`; updates a field
  published by the active draft's typed editor. The host validates the draft,
  field and tagged value. `preview: true` is transient; `preview: false`
  confirms it.
- `plugin.set_program_name`: available to `PLAY` and `CONFIG`; changes the
  active draft's portable program name without exposing plugin document
  internals.
- `plugin.restore_program_preview`: available to `PLAY` and `CONFIG`; restores
  the last confirmed draft after transient previews.
- `plugin.save_program`: available to `PLAY` and `CONFIG`; persists the active
  draft and releases audition focus.
- `plugin.cancel_program`: available to `PLAY` and `CONFIG`; discards the active
  draft and releases audition focus.

These are optional capabilities, not required controls or required screens.
The host validates and transports them; the plugin decides whether and how they
appear. New plugin interfaces should keep musical program selection and editing
in `PLAY`. `CONFIG` is reserved for infrequent setup such as libraries,
resources, compatibility options and plugin diagnostics. Program methods remain
available to `CONFIG` so existing plugin packages keep working while migrating.

## Host-owned resource explorer

Plugins declare the resources they may request:

```toml
[[resources]]
id = "sample-library"
name = "Sample library"
kind = "directory"
required = false
```

A CONFIG surface requests the declared resource:

```json
{
  "protocol": "rackforge.plugin.web@1",
  "kind": "request",
  "request_id": "samples-1",
  "method": "plugin.select_resource",
  "params": { "resource_id": "sample-library" }
}
```

RackForge owns the resulting dialog, platform permissions and navigation. A
successful response contains no native location:

```json
{
  "grant_id": "2yZPpGTf0G4iFvREl_QmDhyF",
  "resource_id": "sample-library",
  "display_name": "My SoundFonts",
  "kind": "directory"
}
```

The browser uses lazy directory loading and opaque handles. Native hosts reject
symbolic links and revalidate every child against its authorized mount.
Windows exposes available drives through the same contract used by Linux and
Raspberry Pi mounts. Android implements the contract over `ContentResolver`:
the first root authorization must use the system Storage Access Framework, but
all navigation within an authorized tree is rendered by RackForge.

After a directory has been granted, a plugin can restore and browse it without
opening the host dialog again:

```json
{
  "protocol": "rackforge.plugin.web@1",
  "kind": "request",
  "request_id": "samples-2",
  "method": "plugin.resource_entries",
  "params": {
    "grant_id": "2yZPpGTf0G4iFvREl_QmDhyF",
    "parent_id": null
  }
}
```

Selecting an entry never exposes its backing path or content URI. The plugin
asks RackForge to stream the selected file into one of its declared file
resources:

```json
{
  "protocol": "rackforge.plugin.web@1",
  "kind": "request",
  "request_id": "samples-3",
  "method": "plugin.load_resource",
  "params": {
    "target_resource_id": "factory-soundfont",
    "grant_id": "2yZPpGTf0G4iFvREl_QmDhyF",
    "entry_id": "Lu9T0t0qRnrM6FXmOsgIBfsR"
  }
}
```

The host derives the plugin and instance identities from the iframe. Desktop
and Raspberry Pi resolve the handle through the confined native grant. Android
copies the document through `ContentResolver` into private app storage before
the same portable plugin loading path is used.

Desktop exposes these native-resource routes only on the loopback listener
used by the embedded application. Enabling its optional LAN HTTP server does
not expose drive discovery, browsing, grants or resource loading. Raspberry Pi
keeps them behind the Web host's normal authorization because that authenticated
Web application is the appliance's primary configuration surface.

The HTTP endpoints below are host-internal transport for the RackForge shell,
not plugin iframe capabilities:

- `GET /api/v1/resources/mounts`
- `GET /api/v1/resources/mounts/{mount_id}/root`
- `GET /api/v1/resources/entries/{parent_id}`
- `POST /api/v1/resources/bind`
- `POST /api/v1/resources/grants`
- `POST /api/v1/resources/browse`
- `POST /api/v1/resources/load`

The backend repeats manifest ownership and resource-kind validation when a
binding is created. A plugin must use the postMessage method; direct endpoint
access is not part of the plugin API.

### Shell resource selections

The RackForge shell also uses the explorer for host-level workflows that do
not belong to a plugin iframe: portable package installation, preset import,
backup restore and future library management. These workflows share a generic
`ResourceSelection` rather than a plugin grant:

```json
{
  "selection_id": "MCd9oGmHfoMXZHZXhDJaQB4N",
  "display_name": "RF-Soundfonts.rfplugin",
  "kind": "file",
  "size": 184320,
  "source": "client_upload",
  "expires_in_seconds": 1800
}
```

There are two interchangeable sources:

- A browser or native client selects a file on its own device. The file is
  streamed into host-owned temporary storage and becomes a `client_upload`.
- The shell browses the RackForge host with opaque entry handles and converts
  one entry into a `host_entry` selection.

Neither source reveals a native path or Android content URI. Selections are
short-lived, single-consumer handles. Host files are never deleted when a
selection is released; uploaded temporary files are deleted after consumption
or expiry. A consumer applies its own validation and size policy. For example,
the resource transport accepts files larger than a portable plugin package,
while the plugin installer still rejects packages above 512 MB.

The authenticated shell transport is:

- `POST /api/v1/resources/uploads?name={display_name}` with a streamed binary body
- `POST /api/v1/resources/selections` with an opaque `entry_id`

Android exposes the same contract to the shared shell through
`rackforge.host@1` method `resource.pick`. The file picker is therefore a host
capability, not an installation-specific action. Feature endpoints consume a
`selection_id`; the explorer itself has no knowledge of plugins, presets or
backups.

RackForge renders this surface with
[`@svar-ui/react-filemanager`](https://github.com/svar-widgets/react-filemanager)
under its MIT license. SVAR is a replaceable view dependency only; the resource
contracts, authorization model, persistence and platform adapters belong to
RackForge.
