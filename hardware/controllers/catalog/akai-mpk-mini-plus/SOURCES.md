# Akai Professional MPK mini Plus — sources

The package describes the keyboard on **Program 1, "MPC"**, which it loads
at power-on ([SX]). No value was measured on hardware.

The User Guide names the controls but gives no MIDI values. The values come
from two Akai sources that agree with each other, with Spectrasonics and
with Bitwig:
- Akai's own Ableton Live "user remote script" for the MPK mini Plus;
- Akai's own FL Studio script for it.

Both are plain-text scripts from Akai's download page for the MPK mini
Plus, read as published.

An earlier version of this file also read the factory program files inside
Akai's MPK mini Plus Program Editor. Akai's software licence forbids reverse
engineering the editor, so those readings were withdrawn on 2026-09-25,
with the joystick they alone described. Nothing below rests on them.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK mini Plus User Guide v1.2 | https://cdn.inmusicbrands.com/akai/attachments/mpkminiplus/MPK%20mini%20Plus%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `5ae891906537208c41f3ac9b2927c1130b1c4d26e9f6a01b70480e995873ccaf` |
| AKA | Akai, "MPK Mini Plus Ableton Remote Script (Beta)", `UserConfiguration.txt` | https://cdn.inmusicbrands.com/akai/attachments/MPK%20mini%20Plus%20Control%20Script.zip | 2026-09-25 | `2a74c3ff7d0024bcdf0387d9a196698bb760c6fff536f041e8bb2065aa9725f0` (zip `f3106273…44f219`) |
| AKF | Akai, "MPK mini Plus FL Studio Beta Script", `config.py` and `device_MPK mini Plus.py` | https://cdn.inmusicbrands.com/akai/attachments/MPK%20mini%20Plus%20FL%20Studio%20Beta%20Script.zip | 2026-09-25 | `28196c58…e6092`, `8f8aedea…6c271` (zip `dc8216ba…8110c`) |
| BW | Bitwig, `MpkMiniPlusControllerExtension.java` and `…Definition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/mpkminiplus (commit `a27f2f4b`) | 2026-09-25 | `4ed73d32…ec5884d`, `215061a3…d11d73` |
| AB | Ableton Live 12 `MPK_mini_Plus` (decompiled; its elements did not decompile) | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPK_mini_Plus | 2026-09-25 | `57805182…f6d7c5` (`__init__.py`) |
| KB-PADS | Akai, "MPK mini Series — How To Program MPK mini Pads in a DAW" | https://support.akaipro.com/en/support/solutions/articles/69000859755 | 2026-09-25 | — (web page) |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the MPK Mini Plus" (relies on Program 1, "PGM1: MPC") | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-mpk-mini-plus | 2026-09-25 | — (web page) |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Port names | Windows `MPK mini Plus`; Mac `MPK mini Plus Port 1` (`Anschluss`, `Puerto`, `Porto`); Linux `MPK mini Plus MIDI 1` | Official software | BW definition |
| The second port | the 5-pin MIDI In/Out as a computer interface | Documented | UG p7, rear panel items 6–7 |
| The MPK mini Plus II | ports `MPK mini Plus II MIDI Port`, `… DAW Port`, `MIDIIN2 (MPK mini Plus II)` | Official software | BW `mpkmk4/MpkMiniPlusControllerExtensionDefinition` |
| Identity Reply, for the record | `F0 7E 7F 06 02 47 54 00 19 00 …` | Official software ×2 | BW `EXPECTED_DEVICE_RESPONSE`; AB `identity_response_id_bytes = (71, 84, 0, 25)`. Not needed: the port name singles the model out |
| USB ids, for the record | Akai 0x09E8, product 0x54 (84) | Official software | AB `controller_id` |
| The factory program | Program 1, "PGM1: MPC", loaded at power-on | A plugin vendor | SX |
| Knobs | CC 70–77, absolute, channel 1 | Official software ×2 (Akai's); a plugin vendor; Official software (Bitwig) | AKA `Encoder1`–`8`, `EncoderChannel` 0, `EncoderMapMode: Absolute`; AKF `ID_KNOB_BASE = 70`, `KNOB_HW_MODE_ABS = 0`; SX "Knob 1: CC#70" … "Knob 8: CC#77"; BW `KNOB_CC = 70` |
| Pads | notes 36–51 on channel 10: bank A 36–43, bank B 44–51 | Official software (Akai's) | AKA `Pad1`–`16Note` 36–51, `PadChannel` 10 |
| Pad rows | pads 1–4 bottom row, 5–8 top row, in either bank | Documented | KB-PADS |
| Pitch wheel, modulation wheel | pitch bend; CC 1; channel 1; fixed | Documented (fixed); Official software (values) | UG p12; AKF `ID_MOD = 1`; BW keys input `b001??`, `e0????` |
| Joystick | modes Pitchbend, Single CC, Dual CC (UG p22); Program 1's setting **not documented publicly** | Documented (the modes) | UG p22. Bitwig sets its own (one CC per axis, 10 and 11). Not declared |
| Transport | << 115, >> 116, Stop 117, Play 118, Rec 119, channel 1 | Documented (the buttons); Official software ×3 (the CCs) | UG items 21–25; AKA `TransportControls`; AKF `config.py`; BW `createButton` (127 press, 0 release) |
| Transport release | Global "Trpt": On, or On/Off | Documented | UG p11. Which one is factory is not documented; a button acts on its press either way |
| Sustain pedal | CC 64, channel 1 | Documented (the input); Official software (the CC) | UG rear panel item 2; BW `b040??` |
| Other buttons | Arp, Tap Tempo, Note Repeat, Full Level, Octave, Bank A/B, Scales, Chords, Shift, Home, Prog Select, Seq Play/Stop and the encoder act on the keyboard itself | Documented | UG pp5–7. Not declared |
| Slots | knobs `control-1`; pads by note; modulation wheel `mod-wheel`; << >> `step` | Convention | catalog README |
| Roles | knobs 1–4 cutoff, resonance, filter envelope amount, LFO rate; 5–8 attack, decay, sustain, release; Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The joystick**: what Program 1 makes it send. A MIDI monitor on the
   hardware settles it.
2. **The Linux and Mac port names** are Bitwig's. No listing has been seen.
3. **The factory transport setting** (On or On/Off).
