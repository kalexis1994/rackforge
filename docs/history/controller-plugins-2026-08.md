# Controller package implementation history — August 2026

This file preserves milestone context without mixing it into the current design.

- The validator, immutable `PackageStore`, trust records, artifact hashes, and
  controller-host CLI landed first on Raspberry Pi.
- Driver supervision moved into the shared package crate so native hosts use the
  same restart and shutdown behavior.
- The Arturia KeyLab Essential mk3 became the first real `process-v1` package,
  with Linux and Windows entrypoints, LITTLE, LED/SysEx support, semantic
  controls, and host bindings.
- Desktop added its store, install flow, TCP bridge, supervisor, and controller
  cards. Android adopted the same store and manifest but kept the KeyLab
  protocol in-process because Android cannot execute downloaded binaries.
- Controller settings began with a host-rendered color setting persisted under
  `controllers/state/<id>/settings.toml`.
- The original design page accumulated completed work, obsolete gaps, and
  future plans. It was split so architecture describes the live contract and
  this page records history.
- `declarative-v1` became the no-code route for generic MIDI controllers.
  `wasm-v1` remains reserved for future portable display/SysEx drivers.
