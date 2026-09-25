# Controller catalog

Declarative `.rfcontroller` packages for third-party controllers. RackForge
ships them and installs them as its own on every host: the appliance's
installer (`rackforge-controller-host install-catalog`), the desktop app and
Android. The list lives in `crates/rackforge-controller-catalog`.

A package here names a controller's controls and their default meanings. It
has no driver and cannot own a screen, so having one installed for hardware
the player does not own costs nothing: it attaches only when a MIDI port it
matches appears. The Controllers page shows a catalog package only once its
keyboard is plugged in or has a map.

## Layout

```
<vendor>-<family>/
  SOURCES.md                      where every value came from
  <model>/rackforge-controller.toml
```

A family with a single model keeps its `rackforge-controller.toml` beside
`SOURCES.md`.

## Rules

1. **The manufacturer's documentation only.**
   - Use user guides, programmer's references, and the maker's own editor as shown in its support articles.
   - Every value is traced in `SOURCES.md`: document, version, page, sha256 of the PDF, and one of four evidence levels (documented, screenshot, convention, not documented).
   - A value that cannot be traced is left out, or declared and marked as an assumption. It is never guessed silently.
2. **The factory state.** Describe what the controller sends out of the box and after a factory reset, on its main MIDI port. Do not describe DAW modes or user presets.
3. **Match narrowly.**
   - Hosts match a package by MIDI port name. The manufacturer's Identity Reply breaks ties.
   - Exclude the DAW, MCU, HUI and MIDIIN2 ports, and sister products (a "Mini", an "FLkey").
   - When one port name pattern covers several models, give each model its `sysex_identity`, taken from the manufacturer's documented Device Inquiry reply. A model that does not answer is then left unclaimed; it is not guessed.
   - When the manufacturer documents no Identity Reply, the port name alone must single the model out, generation included (the "mk3" of a Launchkey Mini [MK3]).
4. **Test it.** `crates/rackforge-controller-catalog` checks that:
   - every package is valid and declarative;
   - the documented Identity Reply picks the right model;
   - the other interfaces and the sister products are never claimed.

## What a package can say beyond its controls

- **Buttons that send MIDI Start, Continue or Stop.** Some keyboards' Play and
  Stop send System Real Time messages, not a Control Change. Declare them with
  `midi = { realtime = "start" }` (no channel) and give them the transport
  actions. Hosts read them on that controller's port only, and never pass them
  to an instrument.
- **Putting the controller in the mode the package describes.** List the
  messages under `[[on_connect]]`. RackForge's own packages send them without
  asking. By default they go to the controller's own port, like the Launch
  Control XL's template change. A controller that changes mode only through its
  DAW port (the Launch Control XL 3) declares that port as a `setup_output`
  endpoint and marks the messages `to = "setup_output"`. The package asks for
  `midi_output`, and for `sysex` when a message is SysEx. The mode is chosen
  only from messages the maker documents.

## Default meanings (roles)

Across the catalog, so that any keyboard feels the same in RackForge:

- **Eight knobs and nine faders** (as the KeyLab Essential mk3):
  - knobs: pulse width, sub level, noise level, filter envelope amount, filter LFO amount, key tracking, LFO delay, amplifier level;
  - faders 1–8: attack, decay, sustain, release, cutoff, resonance, LFO rate, LFO depth;
  - fader 9: master level.
- **Eight knobs, no faders:** cutoff, resonance, filter envelope amount, LFO rate, attack, decay, sustain, release.
- **Rows of knobs and eight faders** (a mixer-style controller): the first row of knobs and faders 1–8 as in the first rule; no master level, and the other rows are left for the player.
- **Transport:** Play and Stop take the transport actions.

## Checking a package on hardware

Plug the keyboard in, open Controllers and move each control; the MIDI
activity must show the messages the package declares. Write any difference
into `SOURCES.md` as a correction, with the firmware version.
