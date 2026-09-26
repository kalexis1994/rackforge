# Akai Professional APC mini mk2 — sources

The package describes the **control port** of the APC mini mk2, with the grid
in Session mode. No value was measured on hardware. Akai's Communication
Protocol tables every inbound message, and Ableton's script agrees with it
value for value.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| CP | APC mini mk2 Communication Protocol v1.0 | https://cdn.inmusicbrands.com/akai/attachments/APC%20mini%20mk2%20-%20Communication%20Protocol%20-%20v1.0.pdf | 2026-09-25 | `38881e0411831a9e67a2896b9dad8d8938b57bda278a6290aa717be4b046d6a7` |
| UG | APC mini mk2 User Guide v1.7 | https://cdn.inmusicbrands.com/akai/apc-mini-mkii/APC%20mini%20mk2%20-%20User%20Guide%20-%20v1.7.pdf | 2026-09-25 | `41b62c908556828e68373fc3dc070bb23e7158c9322203540be71d8164125395` |
| AB | Ableton Live 12 `APC_mini_mk2/elements.py`, `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/APC_mini_mk2 (commit `e83d5192`) | 2026-09-25 | `616a6de0…b83f76`, `41b02135…a69515` |
| BW | Bitwig, `apcmk2/AkaiApcMiniDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/apcmk2 (commit `a27f2f4b`) | 2026-09-25 | `5e8537b2…74394b02` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Ports | port 0 the faders, grid (Session and Drum modes) and buttons; port 1 Note mode | Documented | CP Scene Launch notes, faders "on USB Port 0" |
| Port names | port 0: `APC mini mk2 Control` (Mac), `APC Mini mk2 Control` (Linux), `APC mini mk2` (Windows); port 1: `APC mini mk2 Notes`, `MIDIIN2 (APC mini mk2)` | Official software | BW `listAutoDetectionMidiPortNames` |
| Identity Reply, for the record | `F0 7E … 06 02 47 4F 00 19 …` | Documented; Official software | CP Device Enquiry; AB `identity_response_id_bytes = (71, 79, 0, 25)` |
| Faders 1–8, master fader 9 | CC 48–55 and CC 56, channel 1, absolute | Documented; Official software | CP Channel Faders table; AB `Faders`, `Master_Fader` |
| Clip grid (Session mode) | notes 0–63, channel 1, from the bottom left, 8 wide | Documented; Official software | CP; AB `create_matrix_identifiers(0, 64, width=8, flip_rows=True)` |
| Drum mode | notes 64–127 on channel 10 | Documented; Official software | CP; AB `Drum_Pads`. Not declared: Session mode is the grid's default in Live, and the package describes one mode |
| Track buttons 1–8, Scene Launch 1–8 | notes 100–107, 112–119, channel 1 | Documented; Official software | CP; AB |
| Shift | note 122, the device's modifier | Documented; Official software | CP; AB `add_modifier_button` |
| Held from instruments | every control (`plays = false`): no keys on this port; its grid and buttons send notes | Convention | catalog README (added 2026-09-25) |
| Slots | faders 1–8 `control-1`; the grid's two bottom rows the switches, as a Launchkey's two rows of pads | Convention | catalog README |
| Fn button | Shift | Official software (the device's modifier) | AB |
| Roles | faders 1–8: attack, decay, sustain, release, cutoff, resonance, LFO rate, LFO depth; the master fader: master level | Convention | catalog README (the faders of the first rule) |

## Open questions, until someone with the hardware checks

1. **Which grid mode the unit starts in** when no script sets one. The package
   describes Session mode.
