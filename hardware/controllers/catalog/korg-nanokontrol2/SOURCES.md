# Korg nanoKONTROL2 — sources

The package describes the nanoKONTROL2 in **CC mode with its factory
scene**. No value was measured on hardware.

The unit starts in whatever mode it was last used in, and holding SET MARKER
and CYCLE while connecting engages CC mode ([OM]). Korg documents no message
that picks the mode without writing over the player's scene, so RackForge
sends none.

Korg's MIDI Implementation tables every control's number for its native
mode, on channel 16. Its guides list no factory CC scene. Two independent
programs rely on the factory scene sending the same numbers on channel 1:
- Mixxx, whose mapping ships with the application;
- Overtone, a community music library.

The numbers are the maker's; their channel and CC-mode default rest on
those two.

Korg's documents were downloaded after the user accepted Korg's manual
library License Agreement on 2026-09-25. Only MIDI values are recorded here,
with the documents cited, not their text.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| MI | nanoKONTROL2 MIDI Implementation, Revision 1.00 (2010.12.14), `nanoKONTROL2_MIDIimp.txt` | https://www.korg.com/us/support/download/manual/0/159/2710/ | 2026-09-25 | `87d1d5cbd25bdfce29b77a4b7ce1ebb1e53683a5cd22ab41913df8fb1a54d250` |
| PG | nanoKONTROL2 Parameter Guide E1 | https://www.korg.com/us/support/download/manual/0/159/1913/ | 2026-09-25 | `6d08b0475e7629170748ee13e5b1a54b456cc4c2044db9b03bd74b3bd87d289d` |
| OM | nanoKONTROL2 Owner's Manual (EFGSCJ2) | https://www.korg.com/us/support/download/manual/0/159/1912/ | 2026-09-25 | `f639b322d998de28f797ac943889188f71afecbc8a8189ccfa7205bb294cb09d` |
| MX | Mixxx, `res/controllers/Korg nanoKONTROL 2.midi.xml` | https://github.com/mixxxdj/mixxx (commit `bcfb7956`) | 2026-09-25 | `a19c7e408bc1f1045988c9e4c0ef55fc9d7eab3fd0ff1d12ed6e41bb092c5422` |
| OV | Overtone, `src/overtone/device/midi/nanoKONTROL2.clj` | https://github.com/overtone/overtone (commit `eb531784`) | 2026-09-25 | `e00087f183a2db47c5cfd22d276a4458717dbc6212c803b1906e69fe856d2cf8` |
| MZ | midizap, `examples/nanoKONTROL2.midizaprc` (Linux port name) | https://github.com/agraef/midizap | 2026-09-25 | — |
| OF | openfollow, `openfollow/input/midi.py` (ALSA port name, jack-named) | https://github.com/openfollowapp/openfollow | 2026-09-25 | — (community) |
| BR | brume, `crates/ui-native/src/controllers/nanokontrol2.rs` (ALSA port name, jack-named) | https://github.com/aftertonesignal/brume | 2026-09-25 | — (community) |
| UD | blekenbleu/midi_examples, `nanoKONTROL2.USB.txt` (the USB descriptors: jack strings `nanoKONTROL2 _ CTRL`, `nanoKONTROL2 _ SLIDER/KNOB`) | https://github.com/blekenbleu/midi_examples | 2026-09-25 | — (community) |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Operation modes | DAW modes (Cubase, DP, Live, Pro Tools, SONAR) and CC mode; the unit starts in the last used; SET MARKER + CYCLE at power-up engages CC mode | Documented | OM "Operation mode"; PG Control Mode |
| Port names | input `nanoKONTROL2 SLIDER/KNOB` (Mac; Korg driver `nanoKONTROL2 1 SLIDER/KNOB`), `nanoKONTROL2` (Windows' driver); output `nanoKONTROL2 CTRL` | Documented | OM "nanoKONTROL2 and driver ports" |
| Linux port name | `nanoKONTROL2 MIDI 1`; `nanoKONTROL2 _ CTRL` (`nanoKONTROL2:nanoKONTROL2 _ CTRL 20:0`) where the kernel names ports after the USB jacks, as the Pi's does | Community ×3 | MZ `JACK_IN1`; OF, BR (ALSA listings); UD (the jack strings). The endpoint keeps no word out, so the `CTRL` port is taken on Linux |
| Identity Reply, for the record | `F0 7E 0g 06 02 42 13 01 00 00 …` | Documented | MI 1-2 |
| Control numbers | slider 1–8 CC 0–7; knob 1–8 CC 16–23; S 32–39; M 48–55; R 64–71; Play 41, Stop 42, REW 43, FF 44, REC 45, Cycle 46; Track < 58, Track > 59, Marker Set 60, Marker < 61, Marker > 62 | Documented (native mode, channel 16); Community ×2 (CC mode, channel 1) | MI 4 (2)-(3); MX; OV |
| Channel in CC mode | 1 | Community ×2 | MX (status B0); OV (`:chan 0 :cmd 176`) |
| Buttons | 127 pressed, 0 released when momentary | Documented (the behaviour); Community (the values) | PG Button Behavior; MX; OV |
| Button behaviour in the factory scene | **not documented** (Momentary or Toggle per button) | — | A toggling button is still read press by press |
| Held from instruments | every control (`plays = false`): no keys; its sliders' CC 0-7 are bank select, modulation and volume to an instrument | Convention | catalog README (added 2026-09-25) |
| Slots | sliders `control-1`, knobs `control-2`; S row `switch-1`, M row `switch-2` (no pads); Track `step`, Marker arrows `step-2` | Convention | catalog README |
| Roles | the mixer-style rule: knobs and sliders as the first rule, no master level; Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The factory CC scene** itself: Korg lists no default values.
2. **Which mode a new unit ships in.**
3. **The factory button behaviour** (Momentary or Toggle).
