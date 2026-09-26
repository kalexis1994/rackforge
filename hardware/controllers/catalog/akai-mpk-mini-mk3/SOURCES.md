# Akai Professional MPK mini mk3 — sources

The package describes the keyboard on **Program 1, "PGM:MPC"**, the program
it starts on ([SX]; [FAQ] "Preset 1 ... is configured to align with MPC
Beats"). No value was measured on hardware.

Akai's printed documents give no MIDI values. They come from sources that
rely on the factory program:
- a plugin vendor's hardware profile ([SX]);
- Ableton's script;
- Bitwig's extension, which sends its own program with the same structure.

An earlier version of this file read Akai's editor's factory program files.
Akai's software licence forbids reverse engineering the editor, so those
readings were withdrawn on 2026-09-25. Nothing below rests on them.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| QS | MPK mini Quickstart Guide v1.3 | https://cdn.inmusicbrands.com/akai/mpk3mini/MPK-mini-Quickstart-Guide-v1_3.pdf | 2026-09-25 | `2b81a4c8f9161369ac6ace928d8f64af4bc6e2ed6d6983eba82688b5b0fa3b40` |
| FAQ | Akai, "Akai Pro MPK mini mk3 — Frequently Asked Questions" | https://support.akaipro.com/en/support/solutions/articles/69000798861 | 2026-09-25 | — (web page) |
| KB-PADS | Akai, "MPK mini Series — How To Program MPK mini Pads in a DAW" | https://support.akaipro.com/en/support/solutions/articles/69000859755 | 2026-09-25 | — (web page) |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the MPK Mini mk3" (relies on Program 1's factory settings) | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-mpk-mini-mk3 | 2026-09-25 | — (web page) |
| BW | Bitwig, `MpkMiniMk3ControllerExtension.java` and `…Definition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/mpk_mini_mk3 (commit `a27f2f4b`) | 2026-09-25 | `97b88997…d20b57b` (extension), `89d0375f…a7855d` (definition) |
| AB | Ableton Live 12 MIDI Remote Script `MPK_mini_mkIII` (decompiled): `__init__.py`, `config.py`, `consts.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPK_mini_mkIII (commit `e83d5192`) | 2026-09-25 | `a77321d9…63188`, `4e9c7863…f1b214`, `f3c84e5e…4b70e7` |
| SC | n8-n/projects, SuperCollider `supercollider/mpk-mini3/MpkMini3.sc` (community) | https://github.com/n8-n/projects (commit `89ec0de3`) | 2026-09-25 | `0324cbd584868090f7e4c1a91e12e86622add2a44eced0384887efd293a8e03f` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The factory program | Program 1, "PGM:MPC", loaded at power-on | Documented; a plugin vendor | FAQ; SX |
| Port name | Windows and Mac `MPK mini 3`, Linux `MPK mini 3 MIDI 1`; one port | Official software ×2 | BW definition (1 in, 1 out); AB `model_name="MPK mini 3"` |
| Sister port names | `MPK mini Plus`, `MPK mini Plus MIDI 1`; `MPK mini IV …`; mk2 `MPKmini2`; the first MPK mini `MPK mini` | Official software | BW `mpkminiplus`, `mpkmk4` definitions; AB `MPK_mini_mkII`, `MPK_mini_mkI` `model_name` |
| USB ids, for the record | Akai 0x09E8 (2536), product 0x49 (73) | Official software | AB `controller_id` |
| Identity Reply | **not documented** | — | Matching is by port name only |
| Knobs | CC 70–77, channel 1 | A plugin vendor's profile of the factory program; Official software; Community | SX "Knob 1 (CC#70) … Knob 8 (CC#77)"; BW binds its knobs to 70–77; SC `knobCC = 70..77` |
| Knob mode | absolute (0–127) | **Assumed**; no public document states it | SX maps them to Omnisphere's continuous controls (cutoff, resonance…), which take a CC's value as a position. Against it: SC reads them as relative by default (1 up, anything else down), probably on a keyboard its author reprogrammed. See the open questions |
| Knob layout | K1–K4 top row, K5–K8 bottom row | Official software | BW `initHardwareControlPositions` |
| Pads, note mode | bank A notes 36–43, bank B notes 44–51, channel 10 | Official software ×2 | AB `PAD_TRANSLATION` (36–51, channel 9 zero-based); BW pads 36–43 on channel 9 zero-based |
| Pad rows | pads 1–4 bottom row, 5–8 top row, in either bank | Documented | KB-PADS |
| Pads, CC mode (CC button) | bank A CC 16–23, channel 10 | A plugin vendor; Community | SX "Pad 5 (CC#20)", "Pad 6 (CC#21)"; SC `bankCC = 16..23` on `chan: 9`. Not declared: the package describes note mode |
| Joystick X | pitch bend, channel 1 | Documented | QS item 8 |
| Joystick Y | CC 1 up and down, 0 at centre | Documented (a mod wheel effect; Modulation 2 centres at 0); Official software (CC 1) | QS item 8; FAQ; BW keys input `b001??`, and its program's Y axis (Dual CC, CC 1) |
| Sustain pedal | CC 64, channel 1 | Documented (the input); Official software (the CC) | QS item 2; BW keys note input `b040??` |
| Other buttons | Arp, Tap Tempo, Octave, Bank A/B, CC, Prog Change, Full Level, Note Repeat, Prog Select act on the keyboard itself | Documented | QS items 5–14. Not declared |
| Slots | knobs `control-1` (no faders); pads by note: 40–43, 36–39 → `switch-1`, 48–51, 44–47 → `switch-2`; joystick Y `mod-wheel` | Convention | catalog README, "Slots" |
| Roles | knobs 1–4 cutoff, resonance, filter envelope amount, LFO rate; 5–8 attack, decay, sustain, release | Convention: RackForge's eight-knobs-no-faders rule | catalog README |

## Open questions, until someone with the hardware checks

1. **Knob mode.** Absolute is assumed. If Program 1's knobs turn out to be
   relative, their inputs take `encoder = "relative_twos_complement"`.
2. **Pad aftertouch**: not declared.
3. **A player who switches programs** (Prog Select + pad) gets other CCs.
   The package describes Program 1 only.
4. **The Linux port name** is taken from Bitwig's definition. No `aconnect`
   listing has been seen.
