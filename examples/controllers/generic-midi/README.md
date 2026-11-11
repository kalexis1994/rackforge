# Declarative MIDI controller example

This is a complete declarative controller package in
[schema 2](../../../docs/architecture/controller-packages-v2.md). It contains
only a manifest: no Rust project, native executable, WebAssembly module,
display renderer, or SysEx implementation.

The manifest has three parts:

- **identity** -- `id`, `name`, `vendor` and the endpoint matcher that
  recognises the device;
- **inputs** -- every physical control, with the MIDI message it sends and a
  stable id;
- **meanings** -- the semantic `roles` and host `actions` given to inputs by
  id. They are the standard assignment; a player can map any input
  themselves.

To adapt it:

1. Change `id`, `name`, `vendor` and the endpoint matcher.
2. List your hardware's controls under `[[inputs]]`, with the message each one
   sends.
3. Give roles and actions to the inputs that have an obvious meaning; leave
   the rest unassigned.
4. Verify the directory with `rackforge-controller-host verify`.
5. Install the directory through RackForge's controller package flow.

The same directory is valid on Windows Desktop, Linux x86-64, Raspberry Pi, and
Android. A MIDI input
must still be enabled in RackForge's Audio & MIDI settings. The pure browser
host will use the same manifest once controller-package import is persistent.
If the device is disconnected, the package remains installed and is matched
again when an endpoint with the same stable identity returns.

Use a `process-v1` runtime only when the hardware needs output such as LITTLE,
LEDs, or SysEx. `wasm-v1` remains the planned portable rich-driver runtime.
