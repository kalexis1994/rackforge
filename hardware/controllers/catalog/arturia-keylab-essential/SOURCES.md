# Arturia KeyLab Essential 49, 61 and 88 (first generation) — sources

The package describes the KeyLab Essential in its **DAW map**, which RackForge
recalls on connect, as Ableton and Bitwig do. No value was measured on
hardware. The KeyLab Essential mk3 is a different keyboard, with its own
driver package.

Arturia's manual gives the controls and their DAW-map roles, and the protocol
of its DAW Command Center (Mackie/HUI). It gives no MIDI numbers. Two official
DAW scripts give those, and agree:
- Bitwig's KeyLab Essential extension;
- Ableton Live 12's KeyLab_Essential script.

The DAW map speaks Mackie Control: faders send pitch bend, one channel each,
and buttons send notes. RackForge never lets those reach an instrument
(`plays = false`), mapped or not.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UM | KeyLab Essential User Manual 1.0 | https://downloads.arturia.com/products/keylab-essential-49/manual/keylab-essential_Manual_1_0_0_EN.pdf | 2026-09-25 | `75424cb4c05abdd9e58ad78422a988122edac566009696d5b7cd817737906606` |
| BW | Bitwig, `ArturiaKeylabEssentialControllerExtension.java`, `…ExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/essential (commit `61f96784`) | 2026-09-25 | `139528d5…d3348d`, `9a7c7e87…50ddff8` |
| AB | Ableton Live 12 MIDI Remote Script `KeyLab_Essential` (decompiled): `__init__.py`, `keylab_essential.py`, `sysex.py`, `control_element_utils.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/KeyLab_Essential (commit `0336151d`) | 2026-09-25 | `563e6d6a…854572`, `0e878521…48a4e8`, `a34e66fe…d198`, `300e0c60…815d` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Controls | 9 encoders, 9 faders, 8 pads, Part controls (Next, Prev, Bank), DAW Command Center | Documented | UM 2.5, 2.7–2.10 |
| DAW map | faders 1–8 channel volumes, fader 9 master; encoders pan; Next/Prev shift by 1 track, or by 8 with Bank on | Documented | UM 2.7–2.9 |
| DAW Command Center | Save, Punch, Undo, Metro, Loop, Rewind, Fast-forward, Stop, Play/Pause, Record, in MCU or HUI | Documented | UM 2.10 |
| The three sizes | the same controls | Documented | UM; BW one extension for 49, 61 and 88 |
| Port names | Windows `Arturia KeyLab Essential NN` / `MIDIIN2 (Arturia KeyLab Essenti`, output `MIDIOUT2 (Arturia KeyLab Essent` (cut before the size); Mac `… NN MIDI In` / `… NN DAW In`, `… DAW Out`; Linux `Arturia KeyLab Essential NNMID` / `… #2` | Official software | BW `listAutoDetectionMidiPortNames` |
| USB ids, for the record | Arturia 7285, products 586 (49) and 650 (61) | Official software | AB `controller_id` |
| DAW map, on connect | `F0 00 20 6B 7F 42 02 00 40 51 00 F7` (DAW preset in Mackie mode), then `F0 00 20 6B 7F 42 05 02 F7` (recall memory 2) | Official software ×2 (the recall); one source (the Mackie mode, matching UM 2.10) | BW init; AB `MEMORY_PRESET_SWITCH_MESSAGE_HEADER + (2,)` |
| Faders 1–9 | pitch bend on channels 1–9 | Official software ×2 | BW pitch bend on channels 0–9; AB `Fader_%d` channel = index, `Master_Fader` channel 8 |
| Encoders 1–8 | CC 16–23, channel 1, relative, sign in bit 6 | Official software ×2 | BW `CC - 0x10`, `decodeRelativeCC`; AB `create_ringed_encoder(index + 16)`, `relative_signed_bit` |
| Encoder 9 | **not in either script**; not declared | — | — |
| Wheel | CC 60, relative | Official software | BW `CC == 0x3C` |
| Buttons | notes on channel 1: Play 94, Stop 93, Record 95, Loop 86, Rewind 91, Forward 92, Save 80, Metro 89, Undo 81, Punch 87, Prev 48, Next 49, Prev/Next with Bank 46/47, wheel Prev 98, Next 99, Preset 100, Cat/Char 101, wheel click 84 | Official software ×2 (most); one source each: Save, Preset, Cat/Char (BW), Punch out 88, 46/47 (AB) | BW `onDAWPortMidi`; AB `create_button` |
| Pads in the DAW map | **not declared**: Bitwig reads them on neither port, Ableton on channel 11 of the DAW port | — | They play as the device sends them |
| Held from instruments | faders, encoders, the wheel and every button (`plays = false`) | Convention | catalog README |
| Slots | faders 1–8 `control-1`; encoders `control-2`; Rewind/Forward `step`, Prev/Next `step-2`; Save, Metro, Undo `switch-3.1`, `.3`, `.4` | Convention | catalog README |
| Roles | none: roles take absolute control changes; the slots give every instrument its map | — | — |
| Transport | Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The Linux port names**, and under a kernel that names ports after the
   USB jacks.
2. **The ninth encoder and the pads** in the DAW map.
