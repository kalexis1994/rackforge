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

1. **Traced sources, the manufacturer's first.**
   - Use user guides, programmer's references, and the maker's own editor as shown in its support articles.
   - Where the maker documents no values, the scripts a maker or a DAW vendor ships for the controller are the next source: an Ableton remote script, a Bitwig extension. They encode what the hardware sends. A value from them takes two independent scripts that agree, or is marked as having one.
   - Community reports (forums, MIDI monitor dumps, third-party presets) only corroborate. A value that has only them is marked, and never goes in the messages sent to the controller.
   - Every value is traced in `SOURCES.md`: document or file, version, page or line, sha256, and an evidence level (documented, screenshot, official software, community, convention, not documented).
   - A value that cannot be traced is left out, or declared and marked as an assumption. It is never guessed silently.
   - A controller whose factory state is not documented, but which an official script switches into a known mode, is described in that mode. It uses the same messages, sent on connect (the Oxygen Pro).
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
- **Four knobs and four faders** (the Oxygen Pro Mini): knobs cutoff, resonance, filter envelope amount, LFO rate; faders attack, decay, sustain, release.
- **Transport:** Play and Stop take the transport actions.

## Slots: what each control does in every instrument

Each control a map can use names its slot (`slot = "control-1.3"`). The
plugins' control layouts (`plugins/control-layouts/`) say what each slot does
in each instrument, so a package never lists instruments. RackForge offers
each keyboard the map the two make. The same rules place every keyboard:

- **Continuous rows:**
  - The faders 1–8 (not a master fader) take `control-1`.
  - The first row of knobs or encoders takes the next row, and a second
    encoder page or row the one after.
  - A keyboard without faders puts its first knob row in `control-1`.
- **Pads, by the note they send:**
  - 40–43 and 36–39 take `switch-1.1`–`1.8`.
  - 48–51 and 44–47 take `switch-2.1`–`2.8`.
  - A pad sends the same thing whatever the keyboard. On a 16-pad grid the
    left half is bank 1.
- **A controller without pads:** its first row of eight buttons takes
  `switch-1`, and a second row `switch-2`.
- **Arrows:**
  - Track ◀ ▶ (or ⏪ ⏩) take `step.down` / `step.up`.
  - A second pair (pad ▼ ▲, Send Select ▼ ▲) takes `step-2`.
- **Labelled buttons:** Capture MIDI or Save, Quantise, Metronome or Click,
  and Undo take `switch-3.1`, `3.2`, `3.3` and `3.4`.
- **The modulation wheel** takes `mod-wheel`.
- **Left without a slot:**
  - a button with a host action (Play, Stop);
  - the master fader;
  - fader buttons on a keyboard that has pads;
  - the pitch wheel and the sustain pedal.

## Checking a package on hardware

Plug the keyboard in, open Controllers and move each control; the MIDI
activity must show the messages the package declares. Write any difference
into `SOURCES.md` as a correction, with the firmware version.
