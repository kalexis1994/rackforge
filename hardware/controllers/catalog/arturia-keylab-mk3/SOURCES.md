# Arturia KeyLab mk3 (49, 61, 88) — sources

The package describes the KeyLab mk3 **in its DAW mode, read on its DAW
port**. No value was measured on hardware.

Arturia documents no factory CC map for the ARTURIA program, and its MIDI
Control Center is not a source (its licence forbids reverse engineering). Two
official DAW scripts, Bitwig's and Ableton's, read the keyboard in DAW mode
and agree on every control below except where a row says otherwise.

**The player enters DAW mode on the keyboard**: choose DAW among the three
programs offered at power-up, or press PROG and choose DAW ([UM] p51; [BG]
p4). The DAW protocol is picked under SETTINGS › GLOBAL › DAW PROTOCOL
([UM] p52, p59). Neither script selects the mode by
message: both send only the DAW connection message after the keyboard
answers the Device Inquiry. RackForge sends that message on connect and
nothing else. In the ARTURIA or USER programs the DAW port carries none of
these controls, and the keys still play on the MIDI port.

It is **not** the KeyLab Essential mk3 (`hardware/controllers/arturia-keylab-essential-mk3`,
a driver): that keyboard speaks the protocol on its only port, answers
another identity (`02 00 05`), and uses other messages throughout.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| BW | Bitwig extension, `src/main/java/com/bitwig/extensions/controllers/arturia/keylab/mk3/` (`KeylabMk3ControllerExtensionDefinition`, `MidiProcessor`, `KeylabHardwareElements`, `CcAssignment`, `controls/TouchEncoder`, `controls/TouchSlider`, `controls/RgbNoteButton`) | github.com/bitwig/bitwig-extensions | 2026-09-25 | commit `a27f2f4b3d9a0e9a71b1d1da10ded02d071275c0` |
| BG | Bitwig's "Arturia KeyLab mk3" setup guide (PDF shipped with the extension, 4 pages) | same repository and commit, `src/main/resources/Documentation/Controllers/Arturia/Arturia KeyLab mk3.pdf` | 2026-09-25 | `15b2b37da1fceb8651a58feb15eb1a6e6c7ab4590c05c0b4f1c1fe1deff3c0f5` |
| AB | Ableton Live 12.1 `KeyLab_mk3` remote script, decompiled (`__init__.py`, `elements.py`, `midi.py`) | github.com/chiakibeats/AbletonLive12.1_MIDIRemoteScripts | 2026-09-25 | commit `2c04903d6eb1b26ebd065a8308abbf14f95c39c5` |
| AB2 | Ableton Live 12.0 `KeyLab_mk3`, decompiled separately (`midi.py`, `__init__.py`) | github.com/shakfu/live-midi-scripts, `midi/KeyLab_mk3/` | 2026-09-25 | commit `f3866a25ba44c09a1abd45a5ae6da657ad0bf57f` |
| UM | KeyLab mk3 User Manual (`keylab-mk3_Manual_1_0_2_EN.pdf`; its metadata says 1.0.0, 2024-08-14) | https://dl.arturia.net/products/keylab-49-mk3/manual/keylab-mk3_Manual_1_0_2_EN.pdf | 2026-09-25 | `b51e1e442a440a23021bfe9b9cb8e2db4ff8fb1990435339c065ac0716d5815c` |
| RC | Reason codec for the KeyLab mk3, `src/daw/*.lua` | github.com/aMUSiC/keylab-mk3-reason-codec | 2026-09-25 | commit `8a9f98c56be710ed7f39c99ea68471a9ba53d54e` (community) |
| HD | herdy, `experiments/mk3/PROTOCOL.md` (third-party notes; claims hardware tests) | github.com/dghelm/herdy | 2026-09-25 | commit `906d5d01b1b81fd397ce64f86518dab5e9bbdc3e` (community) |

Line numbers below are the files' at those commits.

## Facts, one by one

Evidence levels: **Official software** (a DAW maker's script), **Documented**
(Arturia's or Bitwig's documents), **Community**, **Convention** (the catalog
README).

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two ports: DAW and MIDI; the protocol is on the DAW port | Windows `MIDIIN2 (KeyLab 49 mk3)` / `MIDIOUT2 (…)` and `KeyLab 49 mk3`; Mac `KeyLab 49 mk3 DAW` / `… MIDI`; Linux `KeyLab 49 mk3 KeyLab 49 mk3 DAW` / `… MID` | Documented; Official software | BG p4; BW `…ExtensionDefinition` port names; AB `__init__.py` ports (the SCRIPT port second) |
| An ALSA listing of the DAW port | `KeyLab 61 mk3:KeyLab 61 mk3 DAW 16:1` | Community | HD |
| Every size has the same panel | 9 encoders, 9 faders | Documented | UM p7 |
| USB product ids | 0x024E / 0x028E / 0x02CE (49/61/88) | Official software | AB `__init__.py` `controller_id`; not used by RackForge's matching |
| Identity Reply | `00 20 6B 02 00 0A …` | Official software | AB `identity_response_id_bytes`. Per-size model bytes: **not found**, so no `sysex_identity`: the port name ("keylab", "mk3", not "essential", the DAW port) singles the family out |
| DAW connection, sent on connect | `F0 00 20 6B 7F 42 00 02 05 01 F7` | Official software ×2 | BW `MidiProcessor` `DAW_CONNECTION` (sent after the Identity Reply); AB `midi.py` `CONNECTION_MESSAGE`; AB2 same |
| DAW mode is entered on the keyboard | at power-up, or PROG → DAW; the protocol under SETTINGS › GLOBAL › DAW PROTOCOL | Documented | UM p51, p52, p59; BG p4 |
| Encoders 1–9 | CC 91, 92, 94, 95, 96, 97, 102, 103, 104, channel 1 | Official software ×2 | BW `KeylabHardwareElements` `ENCODER_CC`; AB `elements.py` 43, 48 |
| Encoder encoding | relative, binary offset (64 = still) | Official software ×2 | BW `TouchEncoder`; AB `MapMode.LinearBinaryOffset` (`elements.py` 19, 43) |
| Faders 1–9 | CC 105–113, channel 1, absolute | Official software ×2 | BW `SLIDER = 0x69` + i; AB `elements.py` 19 (absolute from 105), 44, 48 |
| Encoder touch 1–9 | CC 8–15, 38; 127 touched, 0 let go | Official software ×2 | BW `ENCODER_TOUCH`, `i < 8 ? … : 38`; AB `midi.py` 16 |
| Fader touch 1–4, 6–9 | CC 28–31, 34–37 | Official software ×2 | BW `SLIDER_TOUCH + (i < 4 ? i : i + 1)`; AB `midi.py` 17 |
| **Fader 5 touch** | **CC 33 declared**; Ableton says CC 32 | Official software (BW) + Community ×2; **conflict** with AB | BW (as above); RC; HD. Open question 1 |
| Stop, Play, Record, Tap, Loop, Rewind, Fast-forward, Metro | CC 20–27, channel 1, 127/0 | Official software ×2 | BW `CcAssignment`; AB `elements.py` (buttons from 20) |
| Back; Save, Quantize, Undo, Redo | CC 40; CC 41, 42, 43, 44 | Official software ×2 | BW `CcAssignment`; AB `elements.py` 35 and after |
| The eight buttons around the screen | CC 45–52, channel 1 | Official software ×2 | BW `CONTEXT_BUTTON = 0x2D` + i; AB `elements.py` 25, 41 |
| Main encoder turn | CC 116, binary offset | Official software ×2 | BW `TouchEncoder(40, 0x74, 0x75)`; AB `elements.py` 45 |
| Main encoder press | CC 117 | Official software (BW only) | BW `TouchEncoder(40, 0x74, 0x75)`; Ableton maps no press. Community: RC |
| Pads, DAW bank (BANK+ until DAW) | notes 0–11, channel 10, note-off on release | Official software ×2; Documented (the bank) | BW `RgbNoteButton(i)`; AB `elements.py` 42 (`channels=9`); BG p4 |
| Pads, banks A–D | bank A from note 36 on channel 10; B–D an octave higher each | Documented | UM p20. Which port they use is not stated; the package does not describe them |
| Held from instruments | every input (`plays = false`) | Convention | catalog README. The touch CCs above include CC 11 (expression) and the DAW pads' notes 0–11 are no instrument's |
| Slots | faders 1–8 `control-1`, encoders 1–8 `control-2`; Rewind/Fast-forward `step`; Save, Quantize, Metro, Undo `switch-3.1`–`3.4`; pads none (they send 0–11, not 36–51); screen buttons none (the keyboard has pads) | Convention | catalog README |
| Roles | faders 1–8 as the "eight knobs and nine faders" rule, the master fader master level; encoders none (relative) | Convention | catalog README |
| Transport | Play and Stop take the transport actions | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Fader 5's touch**: CC 33 (Bitwig, and two community sources) or CC 32
   (Ableton). With the other number it reaches an instrument as CC 32, bank
   select's LSB.
2. **Whether the DAW connection message alone changes anything** on the
   keyboard when it is not in DAW mode.
3. **The main encoder's CC 117**: a press, or a touch as Bitwig's class
   name suggests.
4. **Linux names**: Bitwig's guide gives `… MID`, which is truncated; the
   package never reads that port.

## How to check on hardware

In RackForge, put the keyboard in DAW mode (PROG → DAW), open Controllers,
choose the KeyLab mk3 and press **Check controls**. Move every control; the
report lists what arrived against the rows above. A difference goes here as
a correction, with the firmware version.
