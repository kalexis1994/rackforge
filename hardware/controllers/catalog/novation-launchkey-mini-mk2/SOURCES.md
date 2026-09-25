# Novation Launchkey Mini (first and MK2) — sources

The package describes the Launchkey Mini in **Basic Mapping mode**, what it
sends when no DAW has put it in InControl. No value was measured on hardware.

Novation's user guide covers the first Launchkey Mini and the MK2 alike and
tables every message. Bitwig's extension gives the port names on each system.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Launchkey Mini User Guide (2015; "applicable to both the original and MK2 versions") | https://fael-downloads-prod.focusrite.com/customer/dev/s3fs-public/novation/downloads/6945/launchkey-mini-ug-en.pdf | 2026-09-25 | `634c8fb780417ace0c013724f7f5377281a3b2cc7fea56b1ee9a8dd421c7d513` |
| BW | Bitwig, `LaunchkeyMiniControllerExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/novation/launchkey_mini (commit `ae6a9fa0`) | 2026-09-25 | `936c664a…35e03e339` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Versions | the first (grey, orange base) and the MK2 (black, green base) behave alike | Documented | UG p4 |
| Modes | InControl, set by a supported DAW; Basic Mapping otherwise | Documented | UG p4, p8, p13 |
| MIDI channel | 1 at every power-on; changeable with InControl + a pad | Documented | UG p10 |
| Port names | MIDI port: Windows `Launchkey Mini`, Mac `Launchkey Mini LK Mini MIDI`, Linux `Launchkey Mini MIDI 1`; InControl port: `MIDIIN2 (Launchkey Mini)`, `Launchkey Mini LK Mini InControl`, `Launchkey Mini MIDI 2` | Official software | BW `listAutoDetectionMidiPortNames` |
| Rotaries 1–8 | CC 21–28, the keyboard's channel | Documented | UG p11, MIDI Messages Table |
| Pads | notes on channel 10, always in Basic Mapping: top row 40–43, 48–51; bottom row 36–39, 44–47 | Documented | UG p10–11, MIDI Messages Table |
| Buttons | round buttons CC 108 (top), 109 (bottom); arrows up 104, down 105; Track left 106, right 107; 0/127 | Documented | UG p11, MIDI Messages Table. The guide says Track < > work in InControl; they are declared as tabled |
| Octave, InControl | send nothing in Basic Mapping (InControl sends note 10 in InControl mode) | Documented | UG MIDI Messages Table |
| Held from instruments | the six buttons (`plays = false`) | Convention | catalog README |
| Slots | rotaries `control-1` (no faders); pads by note; Track `step`, arrows `step-2` | Convention | catalog README |
| Roles | the eight-knobs-no-faders rule | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Track < > in Basic Mapping mode**: the guide tables their CCs but says
   they work in InControl.
2. **The Linux port names** under a kernel that names ports after the USB
   jacks.
