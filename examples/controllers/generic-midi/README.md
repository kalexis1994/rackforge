# Declarative MIDI controller example

This is a complete `declarative-v1` controller package. It contains only a
manifest: no Rust project, native executable, WebAssembly module, display
renderer, or SysEx implementation.

To adapt it:

1. Change `id`, `name`, and the endpoint matcher.
2. Keep only the semantic controls and host actions your hardware emits.
3. Verify the directory with `rackforge-controller-host verify`.
4. Install the directory through RackForge's controller package flow.

The same directory is valid on Windows Desktop, Linux x86-64, Raspberry Pi, and
Android. A MIDI input
must still be enabled in RackForge's Audio & MIDI settings. The pure browser
host will use the same manifest once controller-package import is persistent.
If the device is disconnected, the package remains installed and is matched
again when an endpoint with the same stable identity returns.

Use `process-v1` only when the hardware needs output such as LITTLE, LEDs, or
SysEx. `wasm-v1` remains the planned portable rich-driver runtime.
