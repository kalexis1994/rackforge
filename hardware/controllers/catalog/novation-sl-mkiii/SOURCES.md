# Novation SL MkIII (49SL, 61SL) — sources

The package describes the SL MkIII **in its InControl view, read on its
InControl port**. No value was measured on hardware.

Novation's Programmer's Reference Guide tables every control the InControl
view sends. Bitwig's extension and Ableton's script, both official software,
read the same numbers. The guide is the source; the scripts corroborate it
and give the port names.

**The player enters the InControl view** with the InControl button ([PR]
p3). Novation documents no message that selects it, so RackForge sends none.
In the keyboard's own template views the InControl port carries none of
these controls, and the keys play on the MIDI port either way ([PR] p15).

**The screens and LEDs are left alone.** The InControl API draws them by
SysEx ([PR] pp. 3–11), which needs a driver that keeps them in step with
what the controls do. Until one exists, they show what the keyboard shows.

Why the package was first left out (2026-09-25): the InControl view needs
the player's button and the screens need a driver. The KeyLab mk3 settled
the first as the player's step, documented as such, and the second as a
driver's job, not the package's.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| PR | SL MkIII Programmer's Reference Guide (16 pages, 2019-09-12) | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/SLMkIII_Programmer's_Guide.pdf | 2026-09-25 | `ee37c6915e43f7884e3b27d64b982ee18c2cadc9ef5d4ea1368c87973724afb6` |
| BW | Bitwig extension, `src/main/java/com/bitwig/extensions/controllers/novation/slmk3/` (`SlMk3ExtensionDefinition`, `SlMk3HardwareElements`, `CcAssignment`, `MidiProcessor`, `control/SlEncoder`) | github.com/bitwig/bitwig-extensions | 2026-09-25 | commit `a27f2f4b3d9a0e9a71b1d1da10ded02d071275c0` |
| AB | Ableton Live 12 `SL_MkIII` remote script, decompiled (`__init__.py`, `elements.py`, `sysex.py`) | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `0336151d3ad8c9c8213ae327d51f8e9381cd18aa` |

The controls table is set as an image-like layout in the PDF; it was read
from pages 14–15 rendered at 110 dpi.

## Facts, one by one

Evidence levels: **Documented** (Novation's guide), **Official software** (a
DAW maker's script), **Convention** (the catalog README).

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The InControl view is entered with the InControl button; all messages go through the InControl USB port | — | Documented | PR p3, p12 |
| Port names | Windows `MIDIIN2 (Novation SL MkIII)` (InControl) and `Novation SL MkIII` (MIDI); Mac `Novation SL MkIII SL MkIII InControl` / `… MIDI`; Linux `Novation SL MkIII SL MkIII InCo` / `… MIDI` | Official software | BW `SlMk3ExtensionDefinition.listAutoDetectionMidiPortNames`; AB `__init__.py` (three inputs, SCRIPT on the second) |
| Channel | 16, for every message from the device | Documented; Official software ×2 | PR p3, p12 (examples `0xbf`, `0x9f`); BW `MidiProcessor` channel `0xF`; AB `elements.py` `DEFAULT_CHANNEL = 15` |
| Rotary knobs 1–8 | CC 21–28, two's complement deltas (1–63 up, 64–127 down) | Documented; Official software ×2 | PR p12, p14; BW `SlEncoder` `0x15 + index`, `createRelative2sComplement…`; AB |
| Faders 1–8 | CC 41–48, 0–127 | Documented; Official software ×2 | PR p12, p14; BW `0x29 + i`; AB `elements.py` `41 + index` |
| Soft buttons 1–24 | CC 51–74, 127 / 0 | Documented; Official software ×2 | PR p12, p14; BW `0x33 + i`, `0x3B + i`; AB `51 + index`, `59 + …` |
| Screen Up/Down, Scene Launch Top/Bottom, Pads Up/Down, Right Soft Buttons Up/Down, Grid, Options, Shift, Duplicate, Clear | CC 81–93 in that order | Documented; Official software | PR p14; BW `CcAssignment` (Shift `0x5B` in `SlMk3HardwareElements`) |
| Track Left, Track Right | CC 102, 103 | Documented; Official software | PR p15; BW `CcAssignment` |
| Rewind, Fast Forward, Stop, Play, Loop, Record | CC 112–117 | Documented; Official software | PR p15; BW `CcAssignment` |
| Pads 1–16 | notes 96–103 and 112–119, velocity, released at velocity 0 | Documented; Official software ×2 | PR p13, p15; BW `0x60 + row * 16 + i`; AB offsets 96, 112 |
| Keys | still sent on the regular port and channel in the InControl view | Documented | PR p15 |
| Identity Reply, for the record | `F0 7E id 06 02 00 20 29 01 01 00 00 …` (family `01 01`, member `00 00`) | Documented (the format); Official software (the values) | PR p16; AB `sysex.py` `DEVICE_FAMILY_CODE`, `DEVICE_FAMILY_MEMBER_CODE`. Not used: the port name singles the product out, and both sizes answer alike (AB `__init__.py`: one USB product id, 257) |
| Held from instruments | every input (`plays = false`) | Convention | catalog README: the InControl port carries controls only |
| Fn button | Shift (`modifier = true`) | Convention | catalog README ("an APC's Shift") |
| Slots | faders `control-1`, knobs `control-2`; Track Left/Right `step`; Pads Up/Down `step-2`; pads and soft buttons none (pads send 96–119, not 36–51) | Convention | catalog README |
| Roles | faders 1–8 as the rule for rows of knobs and eight faders; no master level; knobs none (relative) | Convention | catalog README |
| Transport | Play and Stop take the transport actions | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The Linux port name** on a kernel that names ports after the USB jacks
   (the Pi's): the package accepts any SL MkIII port with "inco" in its
   name, which Bitwig's truncated `… InCo` and a full `… InControl` both
   carry.
2. **What the screens show** in the InControl view with no host drawing on
   them.

## How to check on hardware

Press InControl on the keyboard, open Controllers in RackForge, choose the
SL MkIII and press **Check controls**. Move every control; the report lists
what arrived against the rows above. A difference goes here as a
correction, with the firmware version.
