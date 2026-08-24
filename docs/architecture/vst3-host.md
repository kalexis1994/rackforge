# RackForge VST3 host

RackForge VST3 is a DAW-native instrument edition of the same RackForge plugin
runtime. It is not a wrapper around `rackforge.exe` and it does not open a
second audio device. The DAW owns scheduling, audio I/O, MIDI routing and
latency compensation.

## Runtime boundary

Each VST3 component owns an isolated RackForge instrument instance. Incoming
VST note events retain their sample offset inside the current process block,
the instrument renders stereo audio directly into the buffers supplied by the
DAW, and its opaque RackForge state is stored in the DAW project together with
the host-owned parameters.

The first Windows edition embeds RackForge Concert Grand so a fresh install is
immediately playable. It also uses the user's normal RackForge root, allowing
the VST edition and Standalone to share immutable plugin packages and private
plugin resources without sharing live instances.

The module does not discover an audio interface or a normal MIDI input. Doing
either inside a VST would compete with the DAW and could render the same input
twice.

## `.rfcontroller` and LITTLE

Hardware controller packages remain a supported RackForge capability in the
VST edition. They are process-global rather than component-local because a
physical controller can only have one owner while a DAW can instantiate the
same VST many times.

The controller service therefore follows these rules:

1. One supervisor and one hardware connection exist per DAW process.
2. Exactly one RackForge VST instance owns LITTLE and host-level controller
   actions at a time.
3. Selecting or explicitly focusing another RackForge instance transfers that
   ownership and refreshes the surface.
4. Non-owning instances continue processing MIDI and audio but cannot repaint
   or consume surface controls.
5. The last instance releases the hardware and restores its native surface.

This avoids pretending that LITTLE belongs exclusively to Standalone while
also preventing several VST instances from fighting over the same LEDs and
display. A project with ambiguous controller ownership remains the user's
responsibility, but the host must still serialize access safely.

The initial VST3 audio component establishes the isolated instance and state
contract. The shared controller supervisor is the next integration layer; it
must reuse the existing `.rfcontroller` package runtime rather than add an
Arturia-specific path to the VST module.

## Deliberately absent by default

The embedded HTTP server and RackForge's physical audio/MIDI device settings
are disabled in VST3. They remain Standalone responsibilities. A future VST UI
may use an in-process web surface, but that does not require exposing an HTTP
listener.

## Windows artifact

The supported installable artifact is the VST3 bundle:

`RackForge.vst3/Contents/x86_64-win/RackForge.vst3`

The build also publishes `rackforge-vst3.dll` as a raw module for diagnostics
and packaging. Users should install the complete `RackForge.vst3` directory.
