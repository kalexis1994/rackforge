# Akai Professional MPK mini mk3 — sources

The package describes the keyboard on **Program 1, "PGM:MPC"**, the program
it starts on ([SX]; [FAQ] "Preset 1 ... is configured to align with MPC
Beats"). No value was measured on hardware.

Akai's printed documents give no MIDI values. The values come from the
maker's own factory programs. These ship inside the MPK mini III Program
Editor as the files its "restore factory presets" article loads
([KB-RESTORE]). The editor was extracted, not installed or run. Every field
read from those files agrees with Bitwig's official extension, which sends
the same structure as SysEx. Where the two overlap, they agree value for
value.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PGM | Akai, `Program 1 PGM-MPC.mpkmini3` (and Programs 2–8), inside `AkaiMPKMiniIIIProgramEditorOSX 1.0.4.dmg` → `Akai Professional MPK Mini III Program Editor Installer.pkg` → `/Library/Application Support/inMusic/MPKMiniIIIProgramEditor/…` | https://cdn.inmusicbrands.com/akai/MACEDITORS/AkaiMPKMiniIIIProgramEditorOSX%201.0.4.dmg (linked from akaipro.com Downloads, MPK Mini MK3) | 2026-09-25 | `82796b93d26d651befc2b0c61af6889b272e6e40d56ceb61a2f3cf0a252404f1` (Program 1); DMG `a0a8d6933bf367e599361ff1bdf8ea2d9cf2d52aaf25c2ea1856340cf0d6192e` |
| EG | MPK mini Editor User Guide v1.0 (in the same app bundle, `Contents/Resources`) | as above | 2026-09-25 | `4f094f0814f80819bf30541ed9d49f0898da321293602336598e18f88f5e01a3` |
| QS | MPK mini Quickstart Guide v1.3 | https://cdn.inmusicbrands.com/akai/mpk3mini/MPK-mini-Quickstart-Guide-v1_3.pdf | 2026-09-25 | `2b81a4c8f9161369ac6ace928d8f64af4bc6e2ed6d6983eba82688b5b0fa3b40` |
| FAQ | Akai, "Akai Pro MPK mini mk3 — Frequently Asked Questions" | https://support.akaipro.com/en/support/solutions/articles/69000798861 | 2026-09-25 | — (web page) |
| KB-PADS | Akai, "MPK mini Series — How To Program MPK mini Pads in a DAW" | https://support.akaipro.com/en/support/solutions/articles/69000859755 | 2026-09-25 | — (web page) |
| KB-RESTORE | Akai, "MPK mini Series — How to Restore Factory Presets" (screenshot lists the eight program files) | https://support.akaipro.com/en/support/solutions/articles/69000858651 | 2026-09-25 | — (web page) |
| BW | Bitwig, `MpkMiniMk3ControllerExtension.java` and `…Definition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/mpk_mini_mk3 (commit `a27f2f4b`) | 2026-09-25 | `97b88997…d20b57b` (extension), `89d0375f…a7855d` (definition) |
| AB | Ableton Live 12 MIDI Remote Script `MPK_mini_mkIII` (decompiled): `__init__.py`, `config.py`, `consts.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPK_mini_mkIII (commit `e83d5192`) | 2026-09-25 | `a77321d9…63188`, `4e9c7863…f1b214`, `f3c84e5e…4b70e7` |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the MPK Mini mk3" | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-mpk-mini-mk3 | 2026-09-25 | — (web page) |

## The program file

Each `.mpkmini3` file is a list of records. Every record is: a little-endian
u32 size (the record's size, header included), then u32 0, the field index,
the type (13 for one byte, 14 for two bytes read big-endian, 28 for text),
u32 1 and u32 14, then the value. The fields follow the order of the SysEx
program Bitwig sends (`F0 47 7F 49 64 01 …`), and read as follows:

| Field | Meaning | Program 1 | Bitwig's program |
|---|---|---|---|
| 0 | name | `PGM:MPC` | `PGM:BITWIG` |
| 1 | pad channel (0-based) | 9 | 09 |
| 2 | pad aftertouch | 1 | 01 |
| 3 | keybed / controls channel (0-based) | 0 | 00 |
| 4 | octave (4 = centre) | 4 | the tracked octave, 4 |
| 5–11 | arpeggiator: on, mode, time division, clock, latch, swing, tempo taps | 0 0 4 0 0 0 3 | 00 00 04 01 00 00 03 |
| 12 | tempo | 120 | 00 78 |
| 13 | arpeggiator octave | 0 | 00 |
| 14–16 | joystick X: mode, then its two CCs | 0 0 0 | 00 00 00 |
| 17–19 | joystick Y: mode, then its two CCs | 2 1 1 | 02 01 01 |
| 20–67 | 16 pads × (note, program change, CC) | 36 0 16, 37 1 17 … 51 15 31 | the same |
| 68–107 | 8 knobs × (mode, CC, Lo, Hi, name) | 0 70 0 127 … 0 77 0 127 | 01 (relative), 70–77, 0, 7F |
| 108 | transpose (12 = none) | 12 | 0C |

Two things check this reading independently of Bitwig:
- Field 69, knob 1's CC, is 1 in Program 2 ("PGM:AbletonLive"), with CC 1–8 on its knobs. Those are the CCs Ableton's script reads (`DEVICE_CONTROLS = GENERIC_ENC1 … 8 = 1 … 8`).
- The X axis's mode, 0, is Pitchbend, which is the joystick's documented default ([QS] item 8).

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The factory program | Program 1, "PGM:MPC", loaded at power-on | Documented | FAQ; SX |
| Port name | Windows and Mac `MPK mini 3`, Linux `MPK mini 3 MIDI 1`; one port | Official software ×2 | BW definition (1 in, 1 out); AB `model_name="MPK mini 3"` |
| Sister port names | `MPK mini Plus`, `MPK mini Plus MIDI 1`; `MPK mini IV …`; mk2 `MPKmini2`; the first MPK mini `MPK mini` | Official software | BW `mpkminiplus`, `mpkmk4` definitions; AB `MPK_mini_mkII`, `MPK_mini_mkI` `model_name` |
| USB ids, for the record | Akai 0x09E8 (2536), product 0x49 (73) | Official software | AB `controller_id` |
| Identity Reply | **not documented** | — | Matching is by port name only |
| Knobs | CC 70–77, absolute (mode 0), 0–127, channel 1 | Official software (maker's program); corroborated by a plugin vendor | PGM fields 68–107; SX "Knob 1 (CC#70) … Knob 8 (CC#77)"; BW binds its knobs to 70–77 |
| Knob mode 0 is absolute | Bitwig's own program writes 01 and reads its knobs as relative | Official software ×2 | PGM; BW `createRelative2sComplementValueMatcher`, `" 01 "` in `configureKnob` |
| Knob layout | K1–K4 top row, K5–K8 bottom row | Official software | BW `initHardwareControlPositions` |
| Pads, note mode | bank A notes 36–43, bank B notes 44–51, channel 10 | Official software ×3 | PGM fields 1, 20–67; AB `PAD_TRANSLATION` (36–51, channel 9 zero-based); BW pads 36–43 on channel 9 zero-based |
| Pad rows | pads 1–4 bottom row, 5–8 top row, in either bank | Documented | KB-PADS |
| Pads, CC mode (CC button) | bank A CC 16–23, bank B CC 24–31 | Official software; corroborated by a plugin vendor | PGM; SX "Pad 5 (CC#20)", "Pad 6 (CC#21)". Not declared: the package describes note mode |
| Pads, Prog Change mode | program changes 0–15 | Official software | PGM. Not declared |
| Joystick X | pitch bend, channel 1 | Documented; Official software | QS item 8; PGM field 14 = 0 |
| Joystick Y | Dual CC, CC 1 up and CC 1 down, 0 at centre | Official software; Documented (the mode) | PGM fields 17–19; EG p12 "Dual CC"; FAQ "two CCs per axis. The center axis is 0"; QS "a mod wheel effect" |
| Sustain pedal | CC 64, channel 1 | Documented (the input); Official software (the CC) | QS item 2; BW keys note input `b040??` |
| Other buttons | Arp, Tap Tempo, Octave, Bank A/B, CC, Prog Change, Full Level, Note Repeat, Prog Select change the keyboard's own state and send no message of their own | Documented | QS items 5–14. Not declared |
| Slots | knobs `control-1` (no faders); pads by note: 40–43, 36–39 → `switch-1`, 48–51, 44–47 → `switch-2`; joystick Y `mod-wheel` | Convention | catalog README, "Slots" |
| Roles | knobs 1–4 cutoff, resonance, filter envelope amount, LFO rate; 5–8 attack, decay, sustain, release | Convention: RackForge's eight-knobs-no-faders rule | catalog README |

## Open questions, until someone with the hardware checks

1. **Pad aftertouch.** Field 2 is 1. The editor offers Channel, Polyphonic and
   Off ([EG p10]), and Bitwig listens for both, so which one 1 means is not
   settled. Nothing is declared for it.
2. **A player who switches programs** (Prog Select + pad) gets other CCs.
   Program 2 puts the knobs on CC 1–8, for example. The package describes
   Program 1 only.
3. **The Linux port name** is taken from Bitwig's definition. No `aconnect`
   listing has been seen.
