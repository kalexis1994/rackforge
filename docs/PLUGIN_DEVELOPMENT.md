# External plugin development

RackForge hosts plugins; it does not own their engines, assets, or product
interfaces. Each production plugin lives in an independent repository and is
installed as a versioned `.rfplugin` package.

## Repository boundary

The RackForge repository owns the portable SDK contracts, plugin discovery,
native loader, MIDI/audio routing, session model, controller surfaces, and Web
shell. A plugin repository owns its DSP or sound engine, program model,
declarative `little@1` editor, static Web surfaces, tests, and packaging tools.

Runtime data is never part of a package:

```text
$HOME/rackforge/plugins/<package>/          immutable installed package
$HOME/rackforge/data/plugins/<plugin-id>/  banks and user documents
```

RackForge discovers behavior from the package manifest and runtime descriptors.
Host code must not branch on a plugin ID to draw menus or interpret plugin
state.

## SDK dependency

Until the SDK crates are published independently, plugin repositories pin the
RackForge Git revision they support. For adjacent local checkouts, Cargo source
patches may redirect those dependencies to the working SDK crates. Such patches
belong in ignored `.cargo/config.toml`; release metadata keeps the reproducible
Git pin.

Adding a serialized field that an older host cannot safely consume requires a
Plugin API minor revision. Existing plugins declaring an older minor remain
loadable. New packages declare the first host minor that provides every contract
they use.

## Package contract

A development package is a directory with the following contents. For
distribution, `rackforge-store pack` turns it into a ZIP archive whose file
name ends in `.rfplugin`:

```text
plugin-name-version.rfplugin/
├── rackforge-plugin.toml
├── branding/
│   ├── icon.png
│   ├── banner.png
│   └── splash.png
├── component.wasm
├── metadata/
│   ├── runtime.json
│   ├── parameters.json
│   └── presets.json
└── web/
    └── <static plugin-owned surfaces>
```

New packages use manifest schema 2 and must include a host-rendered visual
identity:

```toml
schema_version = 3
id = "org.example.instrument"
name = "Example Instrument"
short_name = "EXAMPLE"
vendor = "Example Audio"
version = "1.0.0"
description = "A concise description shown before the user installs the plugin."
kind = "instrument"
state_version = 1

[api]
major = 1
minor = 8

[branding]
icon = "branding/icon.png"
banner = "branding/banner.png"
splash = "branding/splash.png"
background_color = "#07131C"
accent_color = "#55E7FF"
```

Instrument and effect packages can declare their main audio layout with Plugin
API `1.9`:

```toml
kind = "effect"
capabilities = ["audio_input", "audio_output", "presets", "state"]

[api]
major = 1
minor = 9

[audio]
input_buses = [{ id = "main", name = "Guitar In", channels = 1, layout = "mono" }]
output_buses = [{ id = "main", name = "Pedalboard Out", channels = 2, layout = "stereo" }]
```

RackForge owns physical interfaces and maps selected channels into these buses.
A plugin must never enumerate ALSA, ASIO, WASAPI or USB devices. It allocates
all delay/reverb work memory during activation and performs no allocation,
locking, logging or filesystem access in its audio callback.

All three files are static, 8-bit RGB or RGBA PNGs. The exact dimensions are
512×512 for the icon, 1600×400 for the banner, and 1920×1080 for the splash.
RackForge fully decodes them while validating the package, rejects filesystem
escapes and excessive file sizes, and exposes only host-owned asset URLs to the
shared UI. See [Plugin branding](PLUGIN_BRANDING.md) for composition rules and
where each asset appears.

`short_name` is required by schema 3, contains one to eight visible ASCII
characters, and is only a responsive fallback. Controllers with enough room
show the full `name`. `description` is optional for compatibility, but new plugins should provide it.
RackForge shows it with the validated banner, name, vendor, version, type, and
package size in a confirmation preview before installation starts.

Schema 1 remains loadable so already published plugins do not break. RackForge
uses its generic plugin identity for those packages; adding any branding field
requires upgrading the whole manifest to schema 2.

The preferred format is `wasm-v1`: a plugin is compiled once against
`rackforge-plugin-sdk`, and RackForge runs the same component on every
compatible host. Native libraries remain supported during the transition, but
they are platform-specific artifacts.

Instruments whose block splits into independent units — a voice-per-core
synthesizer, for example — may additionally declare the versioned
`parallel_render_v1` capability and implement `ParallelProcessor` with
`export_parallel_processor!`. RackForge then distributes the units across its
own audio workers while single-core hosts and the browser keep using the
identical component sequentially. The complete contract lives in
[PARALLEL_RENDER.md](PARALLEL_RENDER.md), with `plugins/parallel-demo-synth`
as the worked five-voice example.

Sound banks, ROMs, credentials, caches, and saved programs are forbidden from
the package. The installer replaces only the immutable package directory and
preserves the plugin data directory.

External files may declare a portable default location without hard-coding a
host path:

```toml
[[resources]]
id = "program-rom"
name = "Program ROM"
kind = "file"
required = false
data_path = "roms/program.bin"
```

RackForge resolves this as
`<data-root>/plugins/<plugin-id>/roms/program.bin`. Explicit user overrides win,
and the host rejects any path that escapes the private plugin directory. A
portable component receives only the resource bytes through the SDK lifecycle;
it never opens that path itself.

Plugins may also declare a ZIP import container whose entries populate several
ordinary file resources:

```toml
[[resources]]
id = "bank-import"
name = "Bank ZIP"
kind = "file"
required = false
import_targets = ["program-rom", "wave-rom"]
```

An importer has no `data_path`: RackForge does not retain the container as a
stand-in for its contents. It ignores entry names, asks a fresh portable plugin
instance to authenticate each candidate against the declared targets, and
persists every recognized file in that target's own `data_path`. Imports are
cumulative, and a later ZIP may complete or replace individual resources.

Plugins whose program list depends on those bytes implement
`Processor::write_program_catalog`. The legacy `write_preset_catalog` spelling
remains source-compatible. RackForge calls it on the control thread after
resource delivery, validates the returned `ProgramCatalog`, and falls back to
`metadata/presets.json` when the optional export is absent or empty. Every
catalog contains at least one PROGRAM; the static catalog should therefore
contain a useful Default/bootstrap entry, not duplicate every private-bank
name. RackForge `.rfpreset` files are a separate host-owned collection.

Plugins declare `config_mode = true` only when they own a separate CONFIG
workflow. A single-window instrument editor belongs in PLAY and leaves the flag
false. RackForge still exposes the CONFIG entry consistently, but renders an
explicit unavailable message on LITTLE and Web instead of guessing or opening
plugin-specific UI. Static Web surfaces are declared independently. LITTLE is
always host-owned; a plugin may contribute declarative program-editor pages,
but does not own the controller layout or its geometry.

## RF-DLS development checkout

The reference external plugin lives in the adjacent
`rackforge-plugin-rf-dls` repository. With both repositories checked out next
to one another:

```bash
cd ../rackforge-plugin-rf-dls
cp .cargo/config.toml.example .cargo/config.toml
cargo test --workspace
bash tools/build-package.sh
bash tools/install-package.sh ./rf-dls-0.1.0.rfplugin
```

RF-DLS owns the DLS engine and all plugin-specific views. RackForge receives a
generic native-program catalog; its `editable` flag is the only fact the host
needs to offer the plugin's program editor for CUSTOM entries. This catalog is
separate from named RackForge presets, which are host-managed opaque state
snapshots described in `architecture/plugin-state-and-presets.md`.

A plugin declaring the `state` capability must serialize every user-visible
setting through `save_state` and restore or reject it atomically through
`load_state`. A native program identifier alone is not a complete state.

## Portable individual-program editing

Portable plugins that create or edit individual programs implement the
program methods on `rackforge_plugin_sdk::Processor`. Return
`PROGRAM_EDIT_BASIC` from `program_editing_capabilities`, optionally combined
with `PROGRAM_EDIT_PREVIEW` and `PROGRAM_EDIT_DECLARATIVE`.

The method inputs and outputs are UTF-8 JSON bytes using the same program types
as the native API. The guest should deserialize the request, update its own
program model, serialize the resulting `PreparedProgram` or editor view into
the supplied destination, and return the number of bytes written. Returning
`None` or `false` rejects the operation atomically. The serialized result and
every request must fit the package's `max_transfer_bytes`; program editing uses
one input and one output buffer of that size.

`preview_program` and `install_program` affect the running plugin instance but
must not open or write files. RackForge owns the final persistence step and
saves the validated program through `PluginStorage`. Consequently a portable
plugin never receives platform paths and uses the identical implementation on
desktop, Android, and Raspberry Pi.
