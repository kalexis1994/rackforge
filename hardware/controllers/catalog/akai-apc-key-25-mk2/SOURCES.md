# Akai Professional APC Key 25 mk2 — sources

The package describes the **control port** of the APC Key 25 mk2, as Akai's
Communication Protocol does. No value was measured on hardware. Akai
publishes a table of every inbound message, and Ableton's script agrees with
it value for value.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| CP | APC Key 25 mk2 Communication Protocol v1.1 | https://cdn.inmusicbrands.com/akai/attachments/APC%20Key%2025%20mk2%20-%20Communication%20Protocol%20-%20v1.1.pdf | 2026-09-25 | `ee06aa845371f72f1c736f548c931577af3e4983d08397dcdc3717d860da5d72` |
| UG | APC Key 25 mk2 User Guide v1.2 | https://cdn.inmusicbrands.com/akai/apc-key-25-mkii/APC%20Key%2025%20mk2%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `b6bd8b1f9c367fd05c39472213c8661b36b34de4407730bf490a2072d81a13ee` |
| AB | Ableton Live 12 `APC_Key_25_mk2/elements.py`, `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/APC_Key_25_mk2 (commit `e83d5192`) | 2026-09-25 | `7edc5eaa…42b093d`, `7d814e99…2ad6e` |
| BW | Bitwig, `apcmk2/AkaiApcKeys25Definition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/apcmk2 (commit `a27f2f4b`) | 2026-09-25 | `1f3b7bf4…d50ee07a` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two ports | port 1 the controls; port 0 the keybed (notes) and sustain (CC 64) | Documented | CP Control Mapping, "Port #" column |
| Port names | control: `APC Key 25 mk2 Control` (Mac, Linux), `MIDIIN2 (APC Key 25 mk2)` (Windows); keys: `APC Key 25 mk2 Keys`, `APC Key 25 mk2` | Official software | BW `listAutoDetectionMidiPortNames` |
| Identity Reply, for the record | `F0 7E <ch> 06 02 47 4E 00 19 …` | Documented; Official software | CP Device Enquiry; AB `identity_response_id_bytes = (71, 78, 0, 25)` |
| Knobs 1–8 | CC 48–55 (0x30–0x37), channel 1, relative | Documented | CP Knobs table |
| Relative encoding | two's complement, accelerated | Official software | AB `MapMode.AccelTwoCompliment` |
| Clip grid | notes 0–39 (0x00–0x27), channel 1, bottom left to top right, 8 wide | Documented; Official software | CP; AB `create_matrix_identifiers(0, 40, width=8, flip_rows=True)` |
| Track buttons 1–8 | notes 64–71 (0x40–0x47) | Documented; Official software | CP; AB |
| Scene Launch 1–5 | notes 82–86 (0x52–0x56) | Documented; Official software | CP; AB |
| Stop All Clips, Play, Record, Shift | notes 81, 91, 93, 98 (0x51, 0x5B, 0x5D, 0x62) | Documented; Official software | CP; AB |
| Shift is a modifier | held for second functions and fine knob moves | Official software | AB `add_modifier_button`, `sensitivity_modifier=shift_button` |
| Oct Down / Oct Up | transpose the keys, send nothing | Documented | CP |
| LEDs | answer notes on port 1 | Documented | CP. RackForge does not drive them |
| Slots | knobs `control-1`; the grid's two bottom rows the switches, as a Launchkey's two rows of pads (upper row: 1.1–1.4, 2.1–2.4; lower row: 1.5–1.8, 2.5–2.8) | Convention | catalog README |
| Fn button | Shift | Official software (it is the device's modifier) | AB |
| Actions | Play `transport_play`; Stop All Clips `transport_stop`, its only stop; their notes are held back from the instruments | Convention | catalog README (Play and Stop take the transport) |
| Roles | none | — | Roles do not read relative encoders yet |

## Open questions, until someone with the hardware checks

1. The Windows name of the control port without Akai's driver: BW lists
   `MIDIIN2 (APC Key 25 mk2)`.
