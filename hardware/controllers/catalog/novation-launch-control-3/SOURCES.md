# Novation Launch Control 3 — sources

`rackforge-controller.toml` describes **Mode 8**, on the controller's MIDI
interface. No value was measured on hardware.

## Why Mode 8

Outside a DAW, the Launch Control 3 sends MIDI only from a Custom Mode (UG
p22). It powers up in standalone mode (PR). Of its eight modes, Mode 8 cannot
be edited: "a default set of the following CCs that send on MIDI channel 16"
(UG p66). It is the one published layout that cannot drift. It plays the part
Mode 16 plays on the Launch Control XL 3, whose package follows the same
approach.

RackForge selects Mode 8 when the controller connects, through its DAW port:
- `9F 0B 7F` turns the feature controls on;
- `B6 1E 15` (surface mode select, CC 1Eh on channel 7) chooses Custom Mode 8.

The guide's table lists Custom Modes 1–4 as 06h–09h and 5–16 as 12h–1Dh, so
mode 8 is 15h. By hand: press **Mode**, the 8th button, and **Mode** again
(UG p20).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Launch Control 3 User Guide v1.1 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launch_control_3_user_guide_v1.1_pdf_en.pdf | 2026-09-25 | `8883ee120b3dc8a354f1cb0f42d37c9bcb35f376ac6fcaf6a9824f6d31249935` |
| PR | Launch Control 3 programmer's reference guide (Novation User Guides): "MIDI on Launch Control 3", "Launch Control 3 programmer's standalone (MIDI) mode", "Launch Control 3 feature Controls" | https://userguides.novationmusic.com/hc/en-gb/sections/33742584375570-Launch-Control-3-programmer-s-reference-guide | 2026-09-25 | — (web pages) |
| BW | Bitwig, `LaunchControlExtensionDefinition.java` (Launch Control 3) | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/novation/launchcontrolxlmk3/definition (commit `1d1109b3`) | 2026-09-25 | `0f8b5898…a94659a61` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Interfaces | MIDI (Custom Modes' output), DAW, two "To DIN Out" | Documented | PR "MIDI on Launch Control 3" |
| Power-up | standalone mode; the DAW interface unused for DAW functions | Documented | PR "standalone (MIDI) mode" |
| Port names | MIDI: Windows `LC3 1 MIDI`; DAW: Windows `MIDIIN2 (LC3 1 MIDI)` / `MIDIOUT2 (LC3 1 MIDI)`, Mac `LC3 1 DAW Out` / `LC3 1 DAW In`, Linux `LC3 1 LC3 1 DAW Out` / `… DAW In` | Official software | BW `listAutoDetectionMidiPortNames`; the number is the device ID, as on the XL 3 |
| Feature controls | on with `9F 0B 7F` on the DAW port; CC on channel 7 | Documented | PR "feature Controls" |
| Surface mode select | CC 1Eh: DAW Mixer 01h, DAW Control 02h, Custom Modes 1–4 06h–09h, 5–16 12h–1Dh | Documented | PR "feature Controls" (the table shared with the XL 3); Mode 8 = 15h |
| Mode 8 encoders | row 1 CC 13–20, row 2 CC 21–28, channel 16 | Documented | UG p66 |
| Mode 8 buttons | CC 37–44, channel 16 | Documented | UG p66 |
| Encoders' values | absolute | Documented, for the XL 3's modes | the XL 3 package's sources (UG p16; PR p10). **Assumed alike** here |
| Other buttons | Page, Track, Record, Play, Solo/Arm, Mute/Select send nothing in standalone mode | Documented | PR "standalone (MIDI) mode" |
| Held from instruments | every control (`plays = false`): no keys, and CC 37–44 are LSBs of volume, pan and expression | Convention | catalog README |
| Slots | row 1 `control-1`, row 2 `control-2` (no faders); buttons `switch-1` | Convention | catalog README |
| Roles | the eight-knobs-no-faders rule on row 1 | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Mode 8's surface-select value, 15h**, follows the table the guide shares
   with the XL 3; the Launch Control 3 has eight modes, not sixteen.
2. **The MIDI port's Mac and Linux names** (Bitwig names only the DAW port
   there). The endpoint takes any "LC3" port that is not the DAW's or a DIN
   one.
3. **Absolute encoders**, as on the XL 3.
