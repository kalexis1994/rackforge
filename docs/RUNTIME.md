# RackForge Runtime

This workspace contains the headless host, shared contracts, platform adapters,
and native-to-portable plugin transition runtime. Product plugins such as
RF-DLS live in separate repositories and have independent versions and release
pipelines.

## Components

| Directory | Responsibility |
| --- | --- |
| `crates/rackforge-control-api/` | Versioned local protocol between Core and control surfaces. |
| `crates/rackforge-session-api/` | Platform-independent session state, commands, events, and revisions. |
| `crates/rackforge-plugin-api/` | Versioned C ABI, manifests, state, parameters, and programs. |
| `crates/rackforge-plugin-runtime/` | Native and portable component loading. |
| `crates/rackforge-core/` | Discovery, validation, instances, sessions, and real-time orchestration. |
| `crates/rackforge-ui/` | Hardware-independent components, focus, layout, and styles. |
| `plugins/gain*/` | Native and portable conformance fixtures. |

Plugin development and packaging are described in
[`PLUGIN_DEVELOPMENT.md`](PLUGIN_DEVELOPMENT.md).

## Contract principles

- No Rust `String`, `Vec`, trait object, or allocator crosses the native ABI.
- C and C++ plugins can use
  `crates/rackforge-plugin-api/include/rackforge_plugin.h`.
- A native library exports only `rackforge_plugin_entry_v1`.
- ABI structures carry `struct_size` and `api_version`.
- Host and plugin must share a major version; a plugin minor version cannot be
  newer than the host.
- Variable metadata crosses the boundary as UTF-8 JSON into host-owned buffers.
- Manifest, runtime descriptor, parameter, state, and program schemas are
  versioned independently.
- The audio thread performs no allocation, I/O, logging, locking, or UI work.
- Plugin state is opaque to RackForge.
- Plugins depend on public API/SDK crates, never private Core modules.

## Package layout

The distributable artifact uses the `.rfplugin` extension:

```text
instrument-0.1.0.rfplugin
└── plugin/
    ├── rackforge-plugin.toml
    ├── branding/              # schema 2 icon, banner, and splash PNGs
    ├── lib/
    │   ├── windows-x86_64/
    │   ├── linux-x86_64/
    │   ├── linux-aarch64/
    │   └── android-aarch64/
    ├── component/
    ├── presets/
    ├── web/
    └── assets/
```

The exact directories present depend on the declared runtime. RackForge
validates the archive, paths, identity, versions, target artifacts, integrity,
and limits before atomically materializing it in the immutable package store.
Libraries are never executed directly from an archive.

Declared paths are relative and may not escape the package through `..`,
absolute paths, or symbolic links.

## External resources

A manifest may declare required files or directories that are not distributed
inside the package:

```toml
[[resources]]
id = "rendered-bank"
name = "Rendered SCVA Bank"
kind = "directory"
required = true
data_path = "banks/rendered"
```

`data_path` is resolved below
`<data-root>/plugins/<plugin-id>/`. An explicit user mapping wins over the
suggested location. Core validates every resource before creating an instance.

Trusted native plugins receive a validated path. Portable components receive
resource operations or bytes through their capability boundary and never gain
general filesystem access.

## Private plugin data

Each plugin owns an isolated root:

```text
<data-root>/plugins/<plugin-id>/
```

RackForge creates and protects the root but does not impose internal names such
as `programs`, `resources`, or `banks`. Relative operations and atomic
writes reject traversal, absolute paths, and escaping links.

Legacy `<data-root>/addons` namespaces are migrated atomically. Existing data
is never overwritten when both old and new locations contain the same plugin.

External resources and private data are different concepts: resources are
dependencies selected by the host; private data is content created and managed
by the plugin.

## Dynamic catalogs

A plugin may publish its catalog after instance creation and resource binding.
This supports user-provided banks whose content is unknown at package build
time.

Native and portable runtimes produce the same validated catalog model. If a
portable component does not export a dynamic catalog, RackForge uses the static
catalog bundled in the package.

Catalog identities are stable. UI ordering or display names may change without
changing the sound/program ID stored in sessions.

## Program model

RackForge owns a common versioned envelope and the plugin owns its payload:

```text
ProgramDocument
  +-- plugin_id
  +-- plugin_version
  +-- schema_version
  +-- name
  +-- payload (opaque to RackForge)
```

The plugin prepares, validates, and migrates the payload. RackForge controls
draft lifecycle, audition, atomic persistence, and recovery snapshots.

Declarative editors expose pages and typed fields identified by opaque field
IDs. A surface sends a field edit to the plugin; it never edits an assumed JSON
path directly.

## Parameters and surfaces

Parameters have stable IDs, units, ranges, defaults, automation rules, and
display metadata. UI pages reference IDs rather than binary offsets.

The same parameter and program state feeds LITTLE, WEB, desktop, and Android.
No surface owns a separate musical copy of the state.

## Lifecycle

```text
discover package
  -> validate manifest and artifacts
  -> load runtime
  -> validate descriptor and schemas
  -> create instance
  -> bind resources and private data
  -> activate
  -> process blocks
  -> deactivate
  -> destroy
```

One loaded plugin may own many independent instances in PLAY and LIVE racks.
The runtime remains loaded while any instance exists.

## Local smoke test

With the required Windows GNU tools on `PATH`:

```powershell
$env:Path = "C:\msys64\ucrt64\bin;$env:Path"
cargo test --workspace
cargo build -p rackforge-gain
cargo run -p rackforge-core -- `
  smoke plugins/gain/package `
  --library target/debug/rackforge_gain.dll
```

Linux ARM64 uses `target/debug/librackforge_gain.so`.

## LIVE runtime

Core owns device routing, bounded MIDI/control queues, plugin blocks, mixing,
and the output backend. Sample selection, voices, sustain, synthesis, and
plugin parameters remain plugin responsibilities.

For every source and MIDI channel, Core retains CC 0–119, pitch bend, and
channel pressure. It replays that state after sound selection and audition
handoff so pedals and wheels preserve their logical position. CC 121 explicitly
clears retained state for that source/channel.

Musical MIDI and control-surface access are independent. Normal MIDI endpoints
may feed Core even when unknown; only a registered driver with an exact layout
match may open a SysEx/display endpoint.

Disconnect recovery sends all-notes-off semantics, clears affected source
state, and supervises reconnection without restarting the host.

## Platform profiles and startup

`rackforge-platform-host detect` reports the binary platform, hardware
profile, and allowlisted capabilities. A Raspberry Pi profile may bind Wi-Fi,
telemetry, real-time audio startup, and controller services without exposing
those facilities directly to plugins.

The startup document stores package references, resources, private-data root,
and a typed audio output profile. Device selection uses stable IDs or USB
identity, never an ephemeral ALSA card number.

```bash
rackforge-core resume "$HOME/rackforge/config/audio.toml"
rackforge-core audio-list
```

The appliance optimizer is optional and reversible:

```bash
platforms/raspberry-pi/scripts/optimize-appliance.sh audit
platforms/raspberry-pi/scripts/optimize-appliance.sh apply
platforms/raspberry-pi/scripts/optimize-appliance.sh rollback
```

Wi-Fi credentials travel through the local privileged host protocol and never
appear in process arguments or logs.

## Temporary audition focus

An editor acquires, renews, and releases an exclusive lease. Core captures the
active program before granting it and restores that program when focus is
released, transferred, disconnected, or expired.

The 15-second watchdog runs on the control plane and sends restoration through
the same bounded audio queue. The audio thread never reads clocks, mutexes, or
UI state.

## Current API scope

The transition API supports:

- instruments, effects, and MIDI processors;
- interleaved `f32` audio and block-positioned short MIDI;
- sample-accurate parameter automation;
- declarative parameters and editor trees;
- stable banks, sounds, and programs;
- plugin-versioned opaque state;
- declared external resources and isolated private data;
- dynamic catalogs;
- native and portable individual-program editing with host-owned persistence;
- stable session instances, commands, events, and revisions;
- bounded synchronization history;
- recoverable audition leases;
- native dynamic loading and sandboxed portable components.

The portable runtime is the long-term compatibility path. Native plugins remain
trusted transition artifacts and must declare every supported target explicitly.
