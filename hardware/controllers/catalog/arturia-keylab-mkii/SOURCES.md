# Arturia KeyLab mkII 49, 61 and 88 — sources

The packages describe the KeyLab mkII in its **DAW preset**, which RackForge
recalls on connect, as Ableton and Bitwig do. No value was measured on
hardware.

Arturia's manual names the DAW mode but gives none of its messages. Two
official DAW scripts give them, and agree value for value:
- Bitwig's KeyLab mkII extension, written for this keyboard;
- Ableton Live 12's KeyLab_mkII script, which builds on its KeyLab_Essential
  script.

The DAW preset speaks a Mackie-style protocol: faders send pitch bend, one
channel each, and buttons send notes. RackForge never lets those reach an
instrument (`plays = false`), mapped or not. The keys, wheels and pedals stay
on the first port and play as on any keyboard.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UM | KeyLab MkII User Manual 2.2 | https://dl.arturia.net/products/keylab-49-mkII/manual/keylab-mkii_Manual_2_2_0_EN.pdf | 2026-09-25 | `d2fafaf629f248f8e0ff2514921276b80bcc5fede476a2130974510254e5b33a` |
| BW | Bitwig, `ArturiaKeylabMkII.java`, `ArturiaKeylabMkIIControllerExtensionDefinition.java`, `ButtonId.java`, `DAWMode.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/arturia/keylab/mk2 (commit `7f7dfc42`) | 2026-09-25 | `8d4942d2…00b877`, `1b49a34a…e5c750`, `4f9d30e2…3cebc`, `8531a6d8…b5c0e` |
| AB | Ableton Live 12 MIDI Remote Scripts (decompiled): `KeyLab_mkII/keylab_mkii.py`; `KeyLab_Essential/keylab_essential.py`, `sysex.py`, `control_element_utils.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts (commit `0336151d`) | 2026-09-25 | `a2aa98e8…26d30c`; `0e878521…48a4e8`, `a34e66fe…d198`, `300e0c60…815d` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Modes | Analog Lab, DAW, User | Documented | UM; Arturia FAQ "KeyLab MkII - General Questions" |
| Ports | first: keys, wheels, pedals; second, "DAW": the DAW preset's controls | Official software ×2 | BW note input on port 0, controls on port 1; AB second inport (SCRIPT) |
| Port names | Windows `KeyLab mkII NN` / `MIDIIN2 (KeyLab mkII NN)`, output `MIDIOUT2 (KeyLab mkII NN)`; Mac `KeyLab mkII NN MIDI` / `KeyLab mkII NN DAW`; Linux `KeyLab mkII NN MIDI 1` / `KeyLab mkII NN MIDI 2` | Official software | BW `listAutoDetectionMidiPortNames` |
| USB ids, for the record | Arturia 7285, products 587, 651, 715 | Official software | AB `controller_id` |
| DAW preset, on connect | `F0 00 20 6B 7F 42 02 00 40 52 02 F7` (DAW protocol: Live), then `F0 00 20 6B 7F 42 05 02 F7` (recall memory 2, the DAW preset) | Official software ×2 (the recall); one source (the protocol) | BW init, `DAWMode.Live = 0x02`; AB `MEMORY_PRESET_SWITCH_MESSAGE_HEADER + (DAW_MEMORY_PRESET_INDEX = 2,)` |
| Faders 1–9 | pitch bend on channels 1–9 | Official software ×2 (1–8 and the master); one source (the ninth as a fader) | BW `createAbsolutePitchBendValueMatcher(index)` for 9 faders; AB `Fader_%d` `MIDI_PB_TYPE` channel = index, `Master_Fader` channel 8 |
| Encoders 1–9 | CC 16–24, channel 1, relative, sign in bit 6 | Official software ×2 (1–8); one source (the ninth) | BW `createEncoder(0x10 + i)` for 9, `createRelativeSignedBitCCValueMatcher`; AB `create_ringed_encoder(index + 16)`, `relative_signed_bit`, for 8 |
| Main wheel | CC 60, relative as the encoders | Official software | BW `createClickEncoder("wheel", 0x3C)` |
| Pads (DAW preset) | notes 36–51 on channel 10, on the DAW port | Official software ×2 | BW `ButtonId.PAD1`–`PAD16`; AB `PAD_IDS` on channel 10 |
| Buttons | notes on channel 1; press at velocity ≥ 64, release below or by note-off. Play 94, Stop 93, Record 95, Loop 86, Rewind 91, Forward 92, Metro 89, Save 74, Undo 81, Punch In 87, Punch Out 88, Read 56, Write 57, Solo 8, Mute 16, Record Arm 0, Previous 48, Next 49, Preset < 98, Preset > 99, Bank 33, wheel click 84, Select Multi 51, Select 1–8 24–31 | Official software ×2, but for Bank (one source) | BW `ButtonId`, `createButton`; AB `create_button` notes, as listed in the manifest's comments. AB reads note 74 as its View button, and 46/47 as bank buttons its KeyLab Essential had |
| Held from instruments | faders, encoders, the wheel and every button (`plays = false`); the pads play | Convention | catalog README |
| Slots | faders 1–8 `control-1`; encoders 1–8 `control-2`; pads by note; Rewind/Forward `step`, Previous/Next `step-2`; Save, Metro, Undo `switch-3.1`, `.3`, `.4`; the ninth fader and encoder none | Convention | catalog README |
| Roles | none: roles take absolute control changes, and these faders send pitch bend, the encoders relative steps. The slots give every instrument its map | — | — |
| Transport | Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The Linux port names under a kernel that names ports after the USB
   jacks.** The endpoint also takes "DAW", the Mac's name for the port.
2. **The ninth encoder (CC 24) and the Bank button (note 33)** rest on Bitwig
   alone.
3. **The keys in the DAW preset**: both scripts read them on the first port,
   so they play whatever RackForge does with the second.
