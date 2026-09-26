# KeyLab Essential mk3 MIDI input map

Read off the hardware on 2026-09-24 with
`cargo run -p rackforge-controller-arturia-keylab-essential-mk3 --example
midi_map`, touching each control on its own. Everything arrives on the main
MIDI port. Channels are one-based here, as the tool prints them; the package
writes them zero-based.

These are the values of the program the driver selects when it takes the
keyboard (`protocol::select_preset(1)`). Arturia publishes what each control
*can* be set to send, not a fixed chart: MIDI Control Center edits them per
program, and a control edited there sends something else.

| Control | Message | Channel | Values | In the package |
|---|---|---|---|---|
| Knobs 1–9 | CC 96–104 | 1 | 0–127, absolute | `encoder-1`…`encoder-9` |
| Faders 1–9 | CC 105–113 | 1 | 0–127 | `fader-1`…`fader-9` |
| Pads 1–4, bank 1 | Notes 40–43 | 11 | velocity, note-off | `pad-1`…`pad-4` |
| Pads 5–8, bank 1 | Notes 36–39 | 11 | velocity, note-off | `pad-5`…`pad-8` |
| Pads 1–4, bank 2 | Notes 48–51 | 11 | velocity, note-off | `pad-b1`…`pad-b4` |
| Pads 5–8, bank 2 | Notes 44–47 | 11 | velocity, note-off | `pad-b5`…`pad-b8` |
| Bank | CC 118 | 1 | 127 / 0 | `bank` |
| Part | CC 119 | 1 | 127 / 0 | `part` (keyboard parts action) |
| Save, Quant, Undo, Redo | CC 40, 41, 42, 43 | 1 | 127 / 0 | `save`, `quant`, `undo`, `redo` |
| ⏪, ⏩, Metronome | CC 25, 26, 27 | 1 | 127 / 0 | `rewind`, `forward`, `metronome` |
| Stop, Play, Record, TAP | CC 20, 21, 22, 23 | 1 | 127 / 0 | driver: transport |
| Loop | CC 24 | 1 | 127 / 0 | driver: FILL |
| OLED buttons 1–4 | CC 44–47 | 1 | 127 / 0 | driver: LITTLE |
| Main encoder, turn | CC 116 | 1 | 65 / 66 clockwise, 63 / 62 counter | driver: LITTLE |
| Main encoder, press | CC 117 | 1 | 127 / 0 | driver: LITTLE |
| Pitch bend wheel | Pitch bend | 1 | full range | `pitch-bend` |
| Modulation wheel | CC 1 | 1 | 0–127 | `modulation` |
| Prog | SysEx `F0 00 20 6B 7F 42 21 11 40 02 00 02 F7` | — | — | not mappable |
| MIDI Ch, Transp −/+, Oct −/+, Arp, Chord, Scale, Hold | nothing | — | — | act inside the keyboard |

The main encoder's turn is relative: 65 is one step clockwise and 66 a
fast turn or two steps; 63 and 62 the same counter-clockwise.
