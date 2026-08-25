# RackForge roadmap

## Current host state and presets

- [x] Versioned opaque references and content-addressed blobs.
- [x] Named RackForge presets scoped by plugin identity.
- [x] Copy-on-load semantics for PLAY and Rack Slots.
- [x] Automatic migration from legacy program IDs to complete state.
- [x] RF-DLS state v3 captures layers, synthesis, envelopes, effects, and gain.
- [ ] [Slot-bound editing with automatic recovery snapshots](https://github.com/kalexis1994/rackforge/issues/19).
- [ ] [Portable manifests for external resources such as samples and banks](https://github.com/kalexis1994/rackforge/issues/20).
- [ ] [Native-format adapters with explicit complete/partial state reporting](https://github.com/kalexis1994/rackforge/issues/21).

## Vision

RackForge is a self-contained, cross-platform musical runtime. The same plugin
must run on Linux ARM64, Windows, macOS, and Android without depending directly
on ALSA, WASAPI, CoreAudio, AAudio, USB APIs, or operating-system paths.

```text
Universal plugin
      |
      v
RackForge Runtime + stable API
      |
      +-- Linux ARM64
      +-- Windows x86-64 / ARM64
      +-- macOS ARM64 / x86-64
      +-- Android ARM64
```

WebAssembly components are the target portable format. The current C ABI and
native libraries remain a transition path until the portable runtime reaches
feature and performance parity.

## Non-negotiable principles

1. Plugins consume only the public RackForge API.
2. The host owns audio, MIDI, storage, networking, controllers, and surfaces.
3. The audio thread never allocates, blocks, performs I/O, or runs UI logic.
4. Plugins receive explicit capabilities instead of general system access.
5. Every public API and persistent format is versioned.
6. State belongs to plugin instances, not only plugin types.
7. LITTLE, WEB, desktop, and Android are projections of the same session.
8. Unknown MIDI devices may play notes but never receive SysEx or display
   commands without a registered controller driver.
9. Layout support is explicit and versioned; it is never inferred from size.
10. Compatibility is maintained through negotiation, migrations, and
    conformance tests.
11. A program payload belongs to the plugin. Surfaces address opaque field IDs
    and never depend on the plugin's internal JSON layout.
12. Instrument plugins and controller packages are built and released
    independently from the RackForge host.

## Target architecture

```text
MIDI inputs -----------+
LITTLE controller -----+
RackForge WEB ----------+--> Command / Event Bus --> Session state
Future automation -----+                              |
                                                       +--> Control plane
                                                       +--> Real-time engine
                                                                |
                                                                v
                                                          Audio backend
```

The session is the single source of truth. A change from WEB must appear on
LITTLE, and an encoder change must reach WEB. Control-plane commands cross into
the audio engine through bounded, non-blocking queues and are applied at safe
block boundaries.

## System layers

### Core

Core discovers and validates packages, creates instances, manages sessions and
performances, persists state, coordinates the real-time engine, and publishes
events. It must not contain product-specific controller or plugin behavior.

### Platform backends

| Area | Linux | Windows | macOS | Android |
| --- | --- | --- | --- | --- |
| Audio | ALSA / PipeWire | WASAPI / ASIO | CoreAudio | AAudio / Oboe |
| MIDI | ALSA Sequencer | Windows MIDI | CoreMIDI | Android MIDI |
| Controllers | Linux transport | Windows transport | macOS transport | USB MIDI |
| WEB | Headless server | Embedded/local | Embedded/local | Embedded WebView |

Backends translate operating-system facilities into shared contracts. Plugins
never see the platform-specific implementation.

### Portable runtime

The Component Model and WIT are intended for lifecycle, metadata, state,
catalogs, commands, and capabilities. The real-time ABI remains deliberately
small:

```text
activate(sample_rate, max_frames)
process(frames, midi_events, audio_buffers)
deactivate()
```

The host preallocates buffers during activation. Processing occurs once per
block and never serializes individual samples.

## Universal plugin API

The public contract covers:

- descriptor, identity, version, and capabilities;
- instance lifecycle and audio configuration;
- block-based MIDI and audio processing;
- parameters, program catalogs, banks, and declarative editor pages;
- state serialization, restoration, and migration;
- external user-provided resources;
- plugin-private storage;
- audition focus;
- commands, events, revisions, and subscriptions;
- LITTLE and WEB view contributions;
- host-provided logging and monotonic time.

Network access, processes, arbitrary files, and direct devices remain separate
capabilities and are denied by default.

## Commands, events, and state

Representative commands include `SelectPlugin`, `SelectProgram`,
`SetParameter`, `SaveProgram`, `BeginAudition`, `EndAudition`,
`SetMasterLevel`, `SetMasterPan`, and `AllNotesOff`.

Accepted changes emit typed events with the affected instance and a monotonic
revision. Clients can reconnect, reject stale edits, and correlate responses
using stable client and command IDs.

## Surfaces and controller packages

`little@1` defines a header, two body rows, four footer actions, and minimal
navigation. Other layouts require their own declared and tested contract.

MIDI input and control-surface access are separate:

```text
Unknown controller
  +-- Note / CC / pitch / pressure --> allowed
  +-- Display / SysEx / host keys --> blocked
```

Physical integration is distributed as an immutable `.rfcontroller` package.
The manifest declares endpoint matchers, layouts, reserved host controls,
permissions, integrity hashes, and per-platform artifacts. Installation assigns
the trust level; a package cannot grant trust to itself.

`process-v1` is the current isolated-process bridge. `wasm-v1` is the target
portable boundary, with RackForge retaining ownership of MIDI and USB handles.

## WEB surface

RackForge owns the server, authentication, sessions, router, global navigation,
theme, device state, and Command/Event Bus. Plugin views mount only inside the
host shell.

Custom views run in a sandboxed iframe and communicate through a typed
`MessagePort` protocol. They never receive the parent DOM, credentials,
router internals, arbitrary sockets, host storage, or the audio thread.

The conservative default remains:

```toml
[web]
enabled = false
bind = "127.0.0.1"
port = 7465
```

LAN exposure requires explicit configuration. Authentication, CSP, CSRF
protection, message limits, and reconnect behavior must be stable before
`web@1` is declared final.

## SDK and tooling

The Rust plugin SDK will provide safe generated bindings, preallocated DSP
buffers, manifest builders, state migrations, and a real-time test harness.
The TypeScript WEB SDK will provide typed commands, subscriptions, navigation,
themes, accessibility, reconnect handling, and an offline simulator.

The CLI will progressively expose:

```text
rackforge new
rackforge build
rackforge test
rackforge validate
rackforge package
rackforge inspect
rackforge dev
```

## Portable `.rfplugin` format

A package contains a versioned manifest, portable component or target-specific
transition artifacts, optional WEB assets, immutable package assets, integrity
metadata, and migration information. It never contains user state.

One archive must install safely on every supported host. If native transition
artifacts are required, they live inside the same package under explicit target
keys. Missing targets fail validation before activation.

## Compatibility policy

- Manifest, host API, state, controller, layout, and WEB protocol versions are
  independent.
- Hosts reject unsupported major versions before loading executable code.
- Minor additions require explicit feature negotiation.
- Persistent schemas require forward migrations and recovery snapshots.
- Every released plugin and controller package must pass a shared conformance
  suite on Windows, Android, and Raspberry Pi.

## Delivery phases

### Completed foundations

- [x] Versioned API crates and native transition ABI.
- [x] Authoritative session, commands, events, and monotonic revisions.
- [x] Plugin package validation and immutable installation.
- [x] PLAY/LIVE mode shared by desktop, Android, WEB, and LITTLE.
- [x] Arturia KeyLab Essential mk3 controller package across desktop, Android,
  Raspberry Pi, and browser-hosted control paths.
- [x] Windows x86-64 standalone and VST3, Linux x86-64, Android ARM64,
  Raspberry Pi ARM64, and browser-demo CI artifacts.
- [x] MIDI disconnect recovery and held-note release.

### Current stabilization milestone: [v0.2.0](https://github.com/kalexis1994/rackforge/milestone/1)

- [x] Required pull-request quality gate covering tests, contracts, and all
  supported build targets.
- [ ] [Qualify MIDI burst, hotplug, audio dropout, screen-lock, and soak reliability](https://github.com/kalexis1994/rackforge/issues/12).
- [ ] [Add Android bridge and lifecycle regression coverage](https://github.com/kalexis1994/rackforge/issues/13).
- [ ] [Split large orchestration modules along stable responsibility boundaries](https://github.com/kalexis1994/rackforge/issues/14).
- [ ] [Freeze `.rfplugin v1` and `.rfcontroller v1` conformance rules](https://github.com/kalexis1994/rackforge/issues/15).
- [ ] [Export privacy-safe diagnostics with device inventory and real-time counters](https://github.com/kalexis1994/rackforge/issues/16).
- [ ] [Define release signing, update, rollback, and key-rotation strategy](https://github.com/kalexis1994/rackforge/issues/17).
- [ ] [Complete the English documentation audit](https://github.com/kalexis1994/rackforge/issues/18).

The milestone and its linked issues are the authoritative operational status;
this document summarizes product direction and delivery order.

### Portable ecosystem

- [ ] Stabilize WIT contracts and the `wasm-v1` runtime.
- [ ] Migrate the reference instrument to the portable runtime.
- [ ] Publish plugin and WEB SDKs with simulators.
- [ ] Add macOS and additional certified controllers.
- [ ] Add a signed repository index and controlled update channels.

## Deliberately deferred decisions

- A VST compatibility bridge is out of scope.
- Arbitrary plugin network and process access is out of scope.
- macOS support follows contract stabilization.
- A public marketplace follows signing, trust, rollback, and conformance.

The immediate goal is reliability and contract stability, not a larger feature
surface.
