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

The portable format in RackForge 0.1.36 is a WebAssembly module using the
`wasm-v1` ABI. Production instruments and effects already use it across
Windows, Linux, Android, Raspberry Pi, and the browser. Hybrid packages can
also carry native builds of the same processor for supported targets.

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

The shipped `wasm-v1` ABI uses WebAssembly exports and host-owned buffers for
lifecycle, metadata, state, programs, and audio processing. The Component Model
and WIT remain future contract work. The real-time processing lifecycle remains
deliberately small:

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

The Rust plugin SDK already provides `Processor` and `ParallelProcessor`
traits, preallocated processing buffers, and portable and native exports.
Independent plugin repositories pin the SDK to a RackForge Git revision.
Standalone SDK publication and simulators remain pending.
[Rust SDK guide](crates/rackforge-plugin-sdk/README.md)

The current Web plugin kit supports host communication and program controls.
The TypeScript WEB SDK remains planned to cover typed commands, subscriptions,
navigation, themes, accessibility, reconnect handling, and an offline simulator.

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

A package contains a versioned manifest, a portable component, optional native
builds and WEB assets, immutable package assets, integrity metadata, and
migration information. It never contains user state.

One archive must install safely on every supported host. The portable
component is always there and runs everywhere; beside it, the same processor
may be built natively for particular platforms under explicit target keys
(`[binaries]`, see [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md)). A host runs a
native build only for a package the release's official set installed, and only
when the build loads; every other case runs the component, so a missing target
costs speed, never the plugin.

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
- [x] Host-owned MIDI Learn and persistent parameter links for enabled MIDI
  inputs, including custom controllers.
  [MIDI Parameter Links](docs/MIDI_PARAMETER_LINKS.md)
- [x] Android lifecycle and soak qualification tools with documented hardware
  and pass/fail thresholds. Retained physical qualification results remain
  part of issue #12.

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

- [x] Ship the `wasm-v1` plugin runtime across supported hosts.
  [Plugin ABI](docs/PLUGIN_ABI.md)
- [x] Migrate reference instruments and effects to the portable runtime.
- [x] Provide the Rust plugin SDK used by independent plugin repositories.
- [ ] Stabilize future Component Model/WIT contracts alongside the shipped ABI.
- [ ] Publish plugin and WEB SDKs with simulators.
- [x] Hybrid packages: one `.rfplugin` with the component and native builds of
  the same processor, held to the component bit for bit in the plugin's CI
  (`rackforge-core compare-native`). RF-Musette runs 1.7-2x faster natively.
- [x] Native builds for parallel processors (`export_parallel_processor!`),
  with equivalence coverage for the composed and scheduled rendering paths.
  [Native parallel test](crates/rackforge-plugin-sdk/tests/native_parallel.rs)
- [ ] Add additional certified controllers.

### macOS

GitHub's macOS runners cost nothing for public repositories, so the work
below needs no Mac to build; a tester with an Apple Silicon Mac runs it.
RackForge's core already builds there (the plugins' CI compiles it on
`macos-14` to compare their native builds), and every hybrid plugin already
ships `macos-aarch64`.

- [ ] Desktop app: macOS dependencies (cpal on CoreAudio, midir on CoreMIDI,
  wry on WKWebView, rfd), and the window, event loop and audio-device paths
  that today distinguish only Windows and Linux.
- [ ] A `.app` bundle (Info.plist, icon) built by CI on `macos-14` and
  published as a zip.
- [ ] VST3 as a macOS bundle (`Contents/MacOS`).
- [ ] Signing and notarization: the Apple Developer Program (US$99 a year)
  before distributing to the public. Until then a tester opens the unsigned
  app once with right-click, Open (or `xattr -dr com.apple.quarantine`).
- [ ] An Intel (`macos-x86_64`) build, if anyone asks for one.

### Distribution

- [ ] Add a signed repository index and controlled update channels.
  Host and package signing, rollback, and key rotation remain tracked by
  [issue #17](https://github.com/kalexis1994/rackforge/issues/17).

## Deliberately deferred decisions

- A VST compatibility bridge is out of scope.
- Arbitrary plugin network and process access is out of scope.
- macOS support follows contract stabilization; its steps are listed under
  Delivery phases.
- A public marketplace follows signing, trust, rollback, and conformance.

The immediate goal is reliability and contract stability, not a larger feature
surface.
