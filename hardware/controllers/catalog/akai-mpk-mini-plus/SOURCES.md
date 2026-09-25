# Akai Professional MPK mini Plus — sources

The package describes the keyboard on **Program 1, "MPC"**. No value was
measured on hardware.

The User Guide names the controls but gives no MIDI values. The values come
from three Akai sources that agree with one another, and with Bitwig:
- the factory programs shipped with Akai's MPK mini Plus Program Editor;
- Akai's own Ableton Live "user remote script" for the MPK mini Plus;
- Akai's own FL Studio script for it.

Each was downloaded from Akai's download page for the MPK mini Plus and read
only. The editor was extracted, not installed or run.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK mini Plus User Guide v1.2 | https://cdn.inmusicbrands.com/akai/attachments/mpkminiplus/MPK%20mini%20Plus%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `5ae891906537208c41f3ac9b2927c1130b1c4d26e9f6a01b70480e995873ccaf` |
| PGM | Akai, `Preset1.mpkminiplus` (and Presets 2–8), inside `AkaiProfessionalMPKminiPlusEditorOSX 1.0.7.dmg` → `…Installer.pkg` → `/Library/Application Support/inMusic/mpkMiniPlusProgramEditor/…` | https://cdn.inmusicbrands.com/akai/MACEDITORS/AkaiProfessionalMPKminiPlusEditorOSX%201.0.7.dmg | 2026-09-25 | `af93e9cc475b7d5c8dfabcd56dc03c9520280dd547bbc45409072be56a782c2e` (Preset 1); DMG `d9a9dffc354911932d2bb542ad6c8eba8947ccf20aa9834370d47621ccff3ccc` |
| AKA | Akai, "MPK Mini Plus Ableton Remote Script (Beta)", `UserConfiguration.txt` | https://cdn.inmusicbrands.com/akai/attachments/MPK%20mini%20Plus%20Control%20Script.zip | 2026-09-25 | `2a74c3ff7d0024bcdf0387d9a196698bb760c6fff536f041e8bb2065aa9725f0` (zip `f3106273…44f219`) |
| AKF | Akai, "MPK mini Plus FL Studio Beta Script", `config.py` and `device_MPK mini Plus.py` | https://cdn.inmusicbrands.com/akai/attachments/MPK%20mini%20Plus%20FL%20Studio%20Beta%20Script.zip | 2026-09-25 | `28196c58…e6092`, `8f8aedea…6c271` (zip `dc8216ba…8110c`) |
| BW | Bitwig, `MpkMiniPlusControllerExtension.java` and `…Definition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/mpkminiplus (commit `a27f2f4b`) | 2026-09-25 | `4ed73d32…ec5884d`, `215061a3…d11d73` |
| AB | Ableton Live 12 `MPK_mini_Plus` (decompiled; its elements did not decompile) | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPK_mini_Plus | 2026-09-25 | `57805182…f6d7c5` (`__init__.py`) |
| KB-PADS | Akai, "MPK mini Series — How To Program MPK mini Pads in a DAW" | https://support.akaipro.com/en/support/solutions/articles/69000859755 | 2026-09-25 | — (web page) |

## The program file

The file has the MPK mini mk3's record structure (see
`../akai-mpk-mini-mk3/SOURCES.md`). Its fields follow the SysEx program
Bitwig sends the Plus (`F0 47 7F 54 64 …`, `PROGRAM_DATA`):

| Field | Meaning | Preset 1 | Bitwig's program |
|---|---|---|---|
| 0 | name | `MPC` | `Bitwig` |
| 1 | pad channel (0-based) | 9 | 09 |
| 3 | keybed / controls channel (0-based) | 0 | 00 |
| 33–35 | joystick X: mode, CC, CC | 2 12 12 | 01 0A 0C |
| 36–38 | joystick Y: mode, CC, CC | 2 2 2 | 01 0B 02 |
| 39–150 | 16 pads × (note, CC, program change, two mode bytes, off colour, on colour) | 36 16 0 0 0 19 1 … 51 31 15 0 0 19 1 | 24 10 00 00 00 12 04 … |
| 151–190 | 8 knobs × (CC, Lo, Hi, mode, name) | 70 0 127 0 "QLINK1" … 77 0 127 0 "QLINK8" | 46 01 7F 01 "Device 1" … |

Bitwig's program makes the knobs relative (mode 1) and reads them so. Akai's
scripts call mode 0 absolute (AKF `KNOB_HW_MODE_ABS = 0`; AKA
`EncoderMapMode: Absolute`). Bitwig sets the joystick to one CC per axis, 10
and 11 (mode 1), and its note input takes CC 10 and CC 11 (`b00A??`,
`b00B??`). So mode 1 is Single CC, and 2 is Dual CC, the editor's third
option (UG p22). The second CCs Bitwig leaves in place, 12 and 2, are the
factory ones.

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Port names | Windows `MPK mini Plus`; Mac `MPK mini Plus Port 1` (`Anschluss`, `Puerto`, `Porto`); Linux `MPK mini Plus MIDI 1` | Official software | BW definition |
| The second port | the 5-pin MIDI In/Out as a computer interface | Documented | UG p7, rear panel items 6–7 |
| The MPK mini Plus II | ports `MPK mini Plus II MIDI Port`, `… DAW Port`, `MIDIIN2 (MPK mini Plus II)` | Official software | BW `mpkmk4/MpkMiniPlusControllerExtensionDefinition` |
| Identity Reply, for the record | `F0 7E 7F 06 02 47 54 00 19 00 …` | Official software ×2 | BW `EXPECTED_DEVICE_RESPONSE`; AB `identity_response_id_bytes = (71, 84, 0, 25)`. Not needed: the port name singles the model out |
| USB ids, for the record | Akai 0x09E8, product 0x54 (84) | Official software | AB `controller_id` |
| Knobs | CC 70–77, absolute, 0–127, channel 1 | Official software ×3 (all Akai's) | PGM 151–190; AKA `Encoder1`–`8`, `EncoderChannel` 0, `Absolute`; AKF `ID_KNOB_BASE = 70`; BW `KNOB_CC = 70` |
| Pads | notes 36–51 on channel 10: bank A 36–43, bank B 44–51; CC mode 16–31; program changes 0–15 | Official software ×2 | PGM 39–150; AKA `Pad1`–`16Note` 36–51, `PadChannel` 10 |
| Pad rows | pads 1–4 bottom row, 5–8 top row, in either bank | Documented | KB-PADS |
| Pitch wheel, modulation wheel | pitch bend; CC 1; channel 1; fixed | Documented (fixed); Official software (values) | UG p12; AKF `ID_MOD = 1`; BW keys input `b001??`, `e0????` |
| Joystick | Dual CC: X CC 12 both ways, Y CC 2 both ways, 0 at centre | Official software (maker's program), with the reading above | PGM 33–38; UG p22. One source for the mode's meaning (Bitwig's program). Declared without a slot |
| Transport | << 115, >> 116, Stop 117, Play 118, Rec 119, channel 1 | Documented (the buttons); Official software ×3 (the CCs) | UG items 21–25; AKA `TransportControls`; AKF `config.py`; BW `createButton` (127 press, 0 release) |
| Transport release | Global "Trpt": On, or On/Off | Documented | UG p11. Which one is factory is not documented; a button acts on its press either way |
| Sustain pedal | CC 64, channel 1 | Documented (the input); Official software (the CC) | UG rear panel item 2; BW `b040??` |
| Other buttons | Arp, Tap Tempo, Note Repeat, Full Level, Octave, Bank A/B, Scales, Chords, Shift, Home, Prog Select, Seq Play/Stop and the encoder act on the keyboard itself | Documented | UG pp5–7. Not declared |
| Slots | knobs `control-1`; pads by note; modulation wheel `mod-wheel`; << >> `step` | Convention | catalog README |
| Roles | knobs 1–4 cutoff, resonance, filter envelope amount, LFO rate; 5–8 attack, decay, sustain, release; Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The joystick.** Two things rest on one source, the reading of Bitwig's
   program: that both X directions send CC 12 and both Y directions CC 2, and
   that mode 2 is Dual CC.
2. **The Linux and Mac port names** are Bitwig's. No listing has been seen.
3. **The factory transport setting** (On or On/Off).
