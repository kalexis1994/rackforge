# Controller plugins: `.rfcontroller` everywhere

**Status: design 2026-08-19; Phase 1 foundations landed the same day.**

Implemented so far:

* `rackforge_control_api::transport` — the shared client transport:
  `RACKFORGE_CONTROL_ADDR` (TCP loopback, every platform) wins over the
  Unix socket path, so a supervisor can always point a spawned driver at
  the right core. This is the seam that makes drivers possible off Linux.
* `rackforge_controller_package::supervise` — the driver supervision loop
  (enumerate, spawn, restart with backoff, shutdown flag, extra env for
  children) extracted from the CLI into the shared crate;
  `rackforge-controller-host serve` now delegates to it, so every host
  runs the same loop.
* Desktop: controller store at `<root>/controllers`,
  `--install-controller <package>` (install-and-exit, official trust for
  local packages; the verifier correctly refuses packages whose declared
  artifacts are missing), and `GET /api/v1/controllers` listing the
  installed packages for the UI.

Landed since: the KeyLab driver already built and answered
`driver-info` on Windows, so the package now declares a
`windows-x86-64` entrypoint (target names are kebab-case — the manifest
validator forbids underscores, and `development_target` was fixed to
match); artifact verification requires the artifact for the platform
installing it rather than every declared target (a Pi build has no
Windows binary and vice versa — one shared manifest, per-platform
bins, integrity checked for whatever is present). The KeyLab installs
on Windows (`RFCONTROLLER_INSTALLED`), and the Plugin Manager shows a
Controllers section: every card carries a kind tag — Instrument or
Controller — with version, trust and device-profile count.

Landed same day, verified against the real hardware: the desktop serves
the framed control protocol on TCP loopback (`control_bridge` in
web.rs, reusing the same `response_for` dispatch every client uses) and
runs the shared supervision loop, handing drivers
`RACKFORGE_CONTROL_ADDR`; the driver's control layer went
cross-platform through the shared transport (its Unix-socket plumbing
stays as the unix fallback; platform-socket menus -- audio/wifi/web
settings -- stay Linux-only no-ops elsewhere); and the desktop's MIDI
capture yields the KeyLab surface port when an enabled controller
package exists (`external_controller_enabled`), keeping the note
endpoints (ALV et al.) for itself. Boot log of record: the supervisor
starts the packaged driver, the desktop captures only the note ports,
and the driver takes the OLED ("OLED bajo control de RackForge") and
registers its host bindings through the bridge.

Phase 2 opened the same day with its first real setting, verified live:
the manifest schema (`[[settings]]` with typed kinds, `color` first),
store persistence (`state/<id>/settings.toml`, written atomically by
`PUT /api/v1/controllers/{id}/settings`, values validated against the
schema), delivery by file-watch (the supervisor hands each driver
`RACKFORGE_CONTROLLER_SETTINGS`; the driver applies changes within a
second -- no shared-enum protocol change needed), and the generic
config page (`/controllers/{id}`, reached from the controller's card):
the KeyLab's `key-light-color` drives all 44 RGB LEDs through
`set_ambient_led_rgb` (8-bit picker values halve to the SysEx 7-bit
range), repainting live as the picker drags. Two host fixes on the way:
the desktop accepts `RegisterHostBindings` (the driver owns its surface
endpoint exclusively, so the reservation is satisfied by construction),
and drivers tolerate hosts that lack it. And one hard-won rule: **a
driver must never outlive its supervisor** -- the supervisor pipes the
child's stdin and the driver exits on EOF, because orphaned drivers
from force-killed hosts were holding MIDI ports hostage.

## Android's topology

Android is the same story with one hard platform constraint: **no
process drivers**. Executing binaries from writable storage is denied
from Android 10 (W^X), and the MIDI transport is Android's Java API,
which the driver binary could not reach anyway (`midir` is excluded on
Android). So on Android the controller logic runs **in-process**: the
Kotlin/Java layer owns MIDI and asks the native library for message
*plans* (JSON arrays of SysEx bytes plus settle delays) rendered by the
same shared `keylab_protocol` crate the process driver uses. That
sharing is the payoff: the ambient-color atomic added for the desktop's
`key-light-color` setting was ALREADY inside Android's renderer.
Everything above the execution layer is STANDARD on Android too: the
bundled KeyLab `.rfcontroller` (the manifest the driver crate already
embeds) auto-installs at boot into the same `PackageStore` layout
(`<filesDir>/controllers`), `controllerCatalog` returns the exact JSON
shape `GET /api/v1/controllers` serves on the desktop,
`controllerApplySettings` validates against the manifest schema and
writes the same `state/<id>/settings.toml`, and the UI lives where it
does everywhere else: the plugin manager, a card with the CONTROLLER
tag, opening a panel derived from the settings schema (color kind →
preview plus RGB bars, 200 ms debounce). Only the runtime differs --
the catalog reports `InProcess`, and applying a setting maps it onto
the shared protocol crate directly instead of a watched file. Process
drivers arrive with `wasm-v1`, which is also what community controllers
need on Android.

Still next: the hardcoded KeyLab library leaves the desktop entirely
(the yield flag and the built-in display path become dead code once the
package is the only route); Android runs the same supervision loop with
the same TCP transport (its KeyLab library link retires the same way);
device matching generalizes from the KeyLab-specific name check to the
manifest's `DeviceMatcher`s. Then Phase 2: the `[[settings]]` schema.

The goal: a controller is a plugin. The Arturia KeyLab is not "the
controller RackForge supports" — it is one `.rfcontroller` package among
any number, installed and updated like an instrument, visible in the
app, and configurable by the user: its sysex programming, its input
mapping, its RGB colors, whatever the package chooses to expose.

## What already exists

More than half of this system is built and shipping on one platform:

* **The package format** (`rackforge-controller-package`): manifest
  `rackforge-controller.toml` (schema v1) with device matchers, driver
  runtime (`process-v1` today, `wasm-v1` reserved), permissions,
  surfaces, host control/action bindings, artifact integrity hashes.
  A `PackageStore` with install records and trust levels
  (official/community), size limits, and a conformance harness.
* **The host CLI** (`rackforge-controller-host`): verify / install /
  activate / serve / exec / conformance.
* **A real driver**: `hardware/keylab-bridge` builds
  `rackforge-arturia-keylab-essential-mk3-driver`, a standalone process
  speaking `PROCESS_DRIVER_PROTOCOL_VERSION 1`.
* **A consumer**: the Raspberry Pi install script verifies and installs
  `org.rackforge.arturia-keylab-essential-mk3.rfcontroller` through the
  CLI. On the Pi, the vision already works.

What breaks the vision today: **the desktop links the KeyLab crate
directly** (`keylab-essential-mk3` in `apps/rackforge-desktop`) and
hardcodes it inside `MidiSupervisor` (`desktop_audio.rs`): device-name
sniffing, display reconciliation, reconnect logic — all specific to one
controller, none of it visible or replaceable.

And the format is missing the piece the user actually asked for:
**user-facing configuration**. The manifest has no settings schema, the
driver protocol has no settings delivery, and no UI shows controllers
at all.

## Design

### Phase 1 — The desktop adopts the package

* Desktop gains a controller store at
  `%LOCALAPPDATA%\RackForge\controllers` (same `PackageStore` the Pi
  uses; no new format).
* `--install-controller <pkg.rfcontroller>` and an install flow in the
  UI, exactly parallel to plugins.
* `MidiSupervisor` stops knowing what a KeyLab is. It becomes a
  **driver supervisor**: for each installed+activated controller whose
  `DeviceMatcher` matches a present MIDI endpoint, spawn its process
  driver and bridge:
  - driver → host: controller events (the existing
    `DesktopControllerEvent` semantics), MIDI passthrough notes.
  - host → driver: display screens (the existing `Screen` channel),
    session context.
  The generic MIDI-input capture (any keyboard, no driver) stays as the
  zero-package fallback.
* The KeyLab library dependency is deleted from the desktop; the
  packaged driver serves both platforms. Parity test: the Pi's
  conformance command runs on desktop CI too.

### Phase 2 — User configuration (the heart of the request)

**Manifest addition** (additive, schema v1 keeps parsing):

```toml
[[settings]]
id = "pad_color_bank_a"
name = "Pad color · bank A"
kind = "color"            # bool | int | float | enum | color | text | sysex
default = "#f3bc7c"
page = "Lighting"

[[settings]]
id = "knob_acceleration"
name = "Knob acceleration"
kind = "enum"
values = ["off", "gentle", "fast"]
default = "gentle"
page = "Input"

[[settings]]
id = "startup_program"
name = "Startup sysex program"
kind = "sysex"            # validated hex, size-capped, permission-gated
default = ""
page = "Advanced"
```

* The host renders these generically (same philosophy as the
  instrument's `parameters.json`: the panel is derived from the schema,
  never hardcoded).
* Values persist in the controller store per id
  (`controllers/state/<id>/settings.toml`), survive updates, and travel
  with the standard state backup.
* **Protocol**: process-v1 gains one message pair —
  `settings { values }` pushed on connect and on every change, and
  `settings_ack { applied, error? }` back. Protocol version bumps to 2;
  v1 drivers keep working (the host simply does not send settings to
  them).
* The driver decides what a setting MEANS (this is the modularity): the
  KeyLab driver maps `pad_color_bank_a` to its RGB sysex writes,
  `startup_program` to a raw program dump on connect. A different
  vendor's package maps its own.
* `kind = "sysex"` is gated by an explicit manifest permission
  (`permissions.sysex = true`) and by the existing trust model:
  community packages show what they request at install time.

### Phase 3 — Rich configuration surfaces (optional, per package)

For controllers whose configuration is visual — a pad grid with
per-pad colors, a macro editor — the schema fader wall is not enough.
The package may ship a **web surface** exactly like instrument plugins
do (`web/config.html`), served by the same asset route family, speaking
a `rackforge.controller.web@1` postMessage protocol:

* `controller.settings` / `controller.set_setting` — the same values
  as Phase 2, so simple and rich UIs never diverge.
* `controller.send_sysex` — permission-gated, rate-limited by the host.
* `controller.status` — connected endpoints, firmware string if known.

The plugins tab lists controllers in their own section ("Controllers")
with connection status; opening one shows the schema panel (Phase 2)
or the package's own surface (Phase 3) when it ships one.

## UI placement

Plugins tab, new "Controllers" section: install, version, trust badge,
connected/disconnected dot, and the configuration panel. Settings >
Audio/MIDI keeps only the endpoint list and a link into the controller
panel. Rationale: install/update/configure is plugin lifecycle, and the
user already knows where plugins live.

## Order of work

1. Desktop store + supervisor + packaged KeyLab (removes the hardcode;
   no user-visible features yet beyond the Controllers list).
2. Settings schema + persistence + protocol v2 + generic panel; the
   KeyLab package exposes its first real settings.
3. Web config surfaces; the KeyLab ships a pad/RGB visual editor as the
   reference implementation.

Each phase lands independently and the Pi keeps working at every step
(the CLI and store are shared code).
