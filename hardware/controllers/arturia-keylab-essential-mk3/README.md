# Arturia KeyLab Essential mk3 controller package

RackForge's first certified controller package implements the text display,
four contextual keys, encoder, LEDs, DAW handshake, health checks, and safe
restoration using the official MIDI/SysEx protocol.

Acquisition, rendering, gesture handling, LED state, and restoration form one
portable state machine shared by Windows, Android, Raspberry Pi, and the
autonomous browser host. The browser transport uses Web MIDI from the HTTPS
page and requests SysEx explicitly; when that permission is unavailable,
ordinary musical MIDI remains available but LITTLE cannot claim the display.
The RGB profile keeps mode buttons, transport, contextual buttons, and all 16
pads at dim blue `(10, 40, 64)`.

The package currently publishes the isolated `process-v1` artifact for
Raspberry Pi while the same I/O-free state machine is prepared for
`wasm-v1`. Desktop and Android embed the shared implementation through their
native host adapters, and the Web build embeds it in the WASI browser host
while JavaScript retains ownership of the browser's MIDI input/output handles.

Semantic RackForge parameters:

- Fader 9, MIDI channel 1 CC 113: `rackforge.master.level`.
- Encoder 9, MIDI channel 1 CC 104: `rackforge.master.pan` (relative).

Reserved action:

- PART, MIDI channel 1 CC 119: `keyboard_parts`.

The host resolves global parameter roles and actions before plugin MIDI routing. The package
implements only `little@1`; other layouts require an explicit certified
implementation.

See [the driver documentation](../../keylab-bridge/README.md) and
[protocol record](../../keylab-bridge/PROTOCOL.md).
