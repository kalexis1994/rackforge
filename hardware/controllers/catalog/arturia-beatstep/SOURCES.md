# Arturia BeatStep — sources

The package describes the first BeatStep (2014) **in CNTRL mode with its
default preset**, on its only MIDI port. No value was measured on hardware.

Arturia's User's Manual tables the default settings of every control in its
chapter 8. That table is the source. Ableton's BeatStep script reprograms
the controls to its own numbers by SysEx, so it describes Live's setup, not
the factory one. It corroborates which physical knobs the table's "Encoder
1–16" are and the pads' notes.

The BeatStep Pro is another product with other defaults (its manual gives
none); it is not described here.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| UM | BeatStep User's Manual 1.0.1 (38 pages) | https://downloads.arturia.com/products/beatstep/manual/BeatStep_Manual_1_0_1_EN.pdf | 2026-09-25 | `d0dd0ac65b6b3bdc245da3a00b0d634feb4800c249a1f47b2a82791b095cce2c` |
| AB | Ableton Live 12 `BeatStep` remote script, decompiled (`__init__.py`, `BeatStep.py`) | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `0336151d3ad8c9c8213ae327d51f8e9381cd18aa` |

The defaults table on UM p37 was read from the page rendered at 110 dpi: its
text layer runs the columns one row out of step.

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The default preset of CNTRL mode | encoders on "a useful variety of MIDI CC numbers", pads on a chromatic scale, transport as MMC Stop and Play, everything on the global channel | Documented | UM p13 |
| Global MIDI channel | 1 by default | Documented | UM p13 |
| Encoders 1–16 | CC 7, 74, 71, 76, 77, 93, 73, 75, 114, 18, 19, 16, 17, 91, 79, 72; absolute | Documented | UM p37 |
| They are the sixteen small knobs, in that order | Ableton programs hardware ids 32–47 with 10, 74, 71, 76, 77, 93, 73, 75, 114, 18, 19, 16, 17, 91, 79, 72: the table's numbers from the second on | Official software (corroborates the order) | AB `BeatStep.py` `HARDWARE_ENCODER_IDS`, `ENCODER_MSG_IDS` |
| The large Level/Rate knob | controls the master level in CNTRL mode; the message is **not documented**, so it is not declared | Documented (the function) | UM p10 |
| Pads 1–8 (top row) | notes 44–51, gate | Documented; Official software | UM p37; AB `PAD_MSG_IDS` |
| Pads 9–16 (bottom row) | notes 36–43, gate | Documented; Official software | UM p37; AB `PAD_MSG_IDS` |
| Stop and Play | MMC Stop (01) and Play (02), device 127 | Documented | UM p37. MMC is SysEx, which a package cannot declare as a control: the transport buttons are left out and take no action |
| Port name | `Arturia BeatStep`; one port | Official software | AB `__init__.py` (`model_name`, one input) |
| Identity Reply | **not documented** | — | The name ("beatstep", not "pro") singles the product out |
| Held from instruments | the sixteen knobs (`plays = false`): no keys; CC 7 is volume | Convention | catalog README |
| The pads play | they are drum pads, as the MPD218's | Convention | catalog README |
| Slots | knobs 1–8 `control-1`, 9–16 `control-2` (no faders); pads by note: 40–43 then 36–39 `switch-1`, 48–51 then 44–47 `switch-2` | Convention | catalog README |
| Roles | knobs 1–8 as the rule for eight knobs and no faders | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The large knob's message** in CNTRL mode.
2. **The Linux port name**, which the package matches by "beatstep" alone.
3. **Whether a unit still holds the default preset**: the player may have
   stored another in its slots; what it sends then is theirs.

## How to check on hardware

Put the BeatStep in CNTRL mode, open Controllers in RackForge, choose it and
press **Check controls**. Turn every knob and hit every pad; the report
lists what arrived against the rows above. A difference goes here as a
correction, with the firmware version.
