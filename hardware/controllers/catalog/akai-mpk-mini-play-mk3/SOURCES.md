# Akai Professional MPK mini Play mk3 — sources

The package describes the keyboard on **Favorite 1 ("FAV 1")**, which is
typically active at power-on ([SX]). No value was measured here.

The User Guide names the controls but gives no MIDI values. The values come
from two sources that agree:
- Spectrasonics' hardware profile, which relies on FAV 1;
- a list of the messages the keyboard sent, taken from the hardware by
  seq66's author, Chris Ahlstrom.

An earlier version of this file read the factory favorites inside Akai's MPK
mini Play mk3 Favorite Editor, and took the port name from the editor's
binary. Akai's software licence forbids reverse engineering the editor, so
those readings were withdrawn on 2026-09-25, with the pads they alone
described. Nothing below rests on them.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK mini Play mk3 User Guide v1.0 | https://cdn.inmusicbrands.com/akai/mpk-mini-play-mk3/MPK_mini_Play_mk3_User_Guide_v1.0.pdf | 2026-09-25 | `bc24e9e8dcc5851b6f46a5bbac9463518b38fc5f4935bddc0265f5aae3cd53d0` |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the MPK Mini Play mk3" | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-mpk-mini-play-mk3 | 2026-09-25 | — (web page) |
| RT | Chris Ahlstrom, "MPK Mini Play Mk3 Outputs" (2025-10-06), `extras/notes/MPK_mini_Play_mk3.text` (community; taken from the hardware) | https://github.com/ahlstromcj/rtl66 (commit `9d816899`) | 2026-09-25 | `b65fb8994cd4e8c1e7e58bfd6a921ff7b696b89d40733edc9ac768e9da74f66a` |
| SQ | seq66 manual, `doc/latex/tex/recording.tex` (`aplaymidi -l`, `arecordmidi -l` listings; community) | https://github.com/ahlstromcj/seq66 (commit `a96ed3c1`) | 2026-09-25 | `0315489ace565e82e678c56f396e9af841e2a059dfffc23a00629890dd1bba31` |
| JZ | JZZ-midi-Gear, `data/models.txt` (Identity Replies collected from devices; community) | https://github.com/jazz-soft/JZZ-midi-Gear (commit `9952b39f`) | 2026-09-25 | `1ec17b488f7f60cc560199a91b25498a6252435196c0ff648e44e2349fe7c03d` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The starting favorite | FAV 1, "typically active by default" at power-on; Favorites + Pad 1 recalls it | A plugin vendor | SX |
| Port name | Linux client `MPK mini Play mk3`, port `MPK mini Play mk3 MIDI 1` | Community (a listing) | SQ `aplaymidi -l`, `arecordmidi -l`; also its `port_mapping.tex` |
| Port name elsewhere | `MPK mini Play mk3` | **Assumed**, from the model's name as the Linux client gives it | — |
| Identity Reply, for the record | `F0 7E 7F 06 02 47 50 00 19 00 …` | Community (collected) | JZ. Not needed: the port name singles the model out |
| Knobs | four 270° knobs, two banks: bank A CC 70–73, bank B CC 74–77, channel 1 | Documented (knobs, banks); a plugin vendor and Community (values) | UG items 16–20; SX "Knob 1 (CC#70)" … "Knob 8 (CC#77)"; RT `0xB0 0x46`…`0x4D` |
| The knobs with the internal sounds | bank A filter, resonance, reverb, chorus; bank B attack, release, EQ low, EQ high; they still send their CCs | Documented (the sounds); Community (the CCs) | UG items 17–20; RT, taken with a drum kit playing. RT notes knob A1 "and some 0xEn" |
| Pads | channel 10; the notes follow the drum kit: the Standard Set sends 36, 38, 42, 46, 40, 45, 51, 49 (bank A) and 60, 62, 63, 64, 58, 75, 56, 77 (bank B) | Community | RT, "Others may emit different numbers". **Not declared**: FAV 1's pads with the internal sounds off are not public |
| Joystick | X pitch bend; Y CC 1; channel 1 | Documented (pitch bend or CCs); Community (values) | UG item 3; RT `0xE0`, `0xB0 0x01` |
| Sustain | the input; CC 64, 0 or 127 | Documented (input); Community (values) | UG rear panel item 4; RT `0xB0 0x40` |
| Internal Sounds button | off: "send and receive MIDI only using the USB port" | Documented | UG item 14 |
| Slots | knobs 1–8 (bank A then B) `control-1`; the joystick's Y `mod-wheel` | Convention | catalog README |
| Roles | the eight-knobs rule: tone on bank A, the amp envelope on bank B | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The pads with the internal sounds off**, on FAV 1. A MIDI monitor
   settles it; they can then be declared by note, as on the MPK mini mk3.
2. **The port name on Windows and a Mac.** The endpoint matches the model's
   name wherever it appears.
3. **The knob A1 pitch-bend messages** RT saw alongside its CC.
