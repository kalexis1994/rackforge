# M-Audio Oxygen Pro 25 / 49 / 61 — sources

The packages describe the keyboards in the **DAW mode RackForge puts them in
when they connect**, not in their factory Preset mode. No value was measured
on hardware.

M-Audio's user guide names every control but lists no default CC. It also
does not say which mode or preset the keyboard powers up in, before or after
a factory reset. Only one community preset file gives the Preset-mode CCs of
the 49's faders. Ableton and Bitwig each ship an official script for these
keyboards. Both switch the keyboard into a mode with the same SysEx messages,
then read the same controls, value for value. The packages send those
messages and read those controls.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Oxygen Pro Series User Guide v1.1 (M-Audio) | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Oxygen%20Pro%20Series%20-%20User%20Guide%20-v1.1.pdf | 2026-09-24 | `8ea31b460f2104c55715d28c45ea2d014fe38461f1e4f44d27bef919bb50d5c9` |
| AB | Ableton Live 12 MIDI Remote Script `Oxygen_Pro` (decompiled): `elements.py`, `midi.py`, `oxygen_pro.py`, `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Oxygen_Pro | 2026-09-24 | `86d8f9a2…dafe7c` (elements.py), `7a78445c…b1f9ea` (midi.py), `f8945871…b64e52` (oxygen_pro.py), `27cab031…58536` (\_\_init\_\_.py) |
| BW | Bitwig's official extension, `controllers/maudio/oxygenpro`: `MidiProcessor.java`, `OxygenCcAssignments.java`, `HwElements.java`, `PadButton.java`, `CcButton.java`, `OxygenProExtensionDefinition.java`, the 25/49/61 definitions | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/maudio/oxygenpro | 2026-09-24 | `90040775…38f70a` (MidiProcessor), `0c06a5e4…265934` (OxygenCcAssignments), `2e93509e…880055` (HwElements), `1486daeb…13b938` (PadButton), `056f42bf…d62761` (OxygenProExtensionDefinition) |
| SX | Spectrasonics, Omnisphere hardware setup pages for the Oxygen Pro 25/49/61 (factory Preset 16) | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-oxygen-pro-49 | 2026-09-24 | — (web page) |

## Facts, one by one

Evidence levels:
- **Documented:** the maker's document states it.
- **Official software:** a script shipped by the maker or by a DAW vendor encodes it.
- **Community:** a third party reports it.
- **Not documented**, and **Assumption**, as in the other packages.

Where **Official software** has two independent sources, both are cited.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Ports, in this order: USB MIDI (keys, pads and controls in Preset mode, clock), MIDI DIN, Mackie/HUI (controls and pads in DAW mode), Editor | Windows: `Oxygen Pro ##`, `MIDIIN2/3/4 (Oxygen Pro ##)`, `MIDIOUT2/3/4 (…)`; macOS: `USB MIDI`, `MIDI DIN`, `MACKIE/HUI`, `EDITOR` | Documented | UG p110 |
| The script's port is the third | Ableton reads its script port as the third input and output; Bitwig autodetects `MIDIIN3 (Oxygen Pro %s)` / `MIDIOUT3 (…)` on Windows and Linux, and `Oxygen Pro %s Mackie/HUI` on a Mac | Official software ×2 | AB `__init__.py` (third inport/outport marked SCRIPT); BW `OxygenProExtensionDefinition` |
| Linux names | `Oxygen Pro Mini Mackie/HUI` and so on, for the Mini, which shares the naming; Bitwig uses the Windows names on Linux | Community (Baeldung `aconnect -i`); Official software (BW), which disagree | The matcher accepts both `midiin3` and `mackie` |
| Mode messages | `F0 00 01 05 7F 00 00 <cmd> 00 01 <val> F7`: 6D=02 (Live firmware mode), 6E=02, 6E=07 (record, then device control mode), 6B=01, 6C=03 (LED control to the host) | Official software ×2 | AB `midi.py`, `oxygen_pro.py` `on_identified`; BW `MidiProcessor.initSysexMessages` |
| RackForge leaves out 6B and 6C | The keyboard keeps its own LEDs | Assumption: RackForge does not drive LEDs; a host that took them and never lit them would leave the pads dark | — |
| Knobs 1–8 | CC 22–29, channel 1, absolute | Official software ×2 | AB `Knob_n = 22 + n`, `MapMode.absolute`; BW `KNOB_1 = 0x16` |
| Faders 1–8 (49, 61) | CC 12–19, channel 1 | Official software ×2 | AB `Fader_n = 12 + n`; BW `SLIDER_1 = 0x0C` |
| Master fader (the 25's only fader) | CC 41, channel 1 | Official software ×2 | AB `Master_Fader`; BW `MASTER_CC = 0x29`, `OxyConfig(8, false, true, true)` for the 25 |
| Fader buttons (49, 61) | CC 32–39, channel 1; 127 on press, 0 on release | Official software; the two disagree on one point | BW `TRACK_1 = 0x20`, `CcButton` matches 127 and 0; AB declares them `is_momentary=False`. They take no slot, so the disagreement touches no map |
| Pads | notes on channel 1: top row 40 41 42 43 48 49 50 51, bottom row 36 37 38 39 44 45 46 47 | Official software ×2 | AB `pad_ids`; BW `PAD_NOTE_NR`, `PadButton` on channel 0 |
| Buttons beside the pad rows | CC 107, 108 | Official software ×2 | AB `Scene_Launch_Button_n = 107 + n`; BW `SCENE_LAUNCH1/2` |
| Transport | Loop 114, Rewind 115, Fast-forward 116, Stop 117, Play 118, Record 119, channel 1 | Official software ×2 | AB `elements.py`; BW `OxygenCcAssignments` |
| Bank < >, Metronome, Back | 110, 111, 106, 104 | Official software ×2 | same |
| Buttons send 127 on press and 0 on release | — | Official software | BW `CcButton` |
| Not declared: the encoder (CC 103) | its relative encoding | **Conflict:** Ableton reads it as `relative_signed_bit` (65 is −1), Bitwig as centred on 64 (65 is +1), so the same turn goes opposite ways | AB `Encoder`; BW `createMainEncoder` |
| Not declared: Shift (CC 105, channel 13), the mode buttons (CC 57–61 and 83–87, channel 16), encoder push (102), Preset and DAW (112, 113) | — | Official software | They change what the keyboard or the host does. **PRESET or DAW, pressed, takes the keyboard out of this mode** until it is connected again |
| Identity Reply | M-Audio `00 01 05`, family LSB 00; model bytes not documented | Official software | AB `product_id_bytes = (0, 1, 5, 0)`. The size in the port name tells the models apart instead |
| Mod wheel, pitch bend, sustain, aftertouch | on the first port, with the keys | Documented (mod wheel CC 1, UG p14) | Not in the package: it reads the third port |
| Preset mode: knobs of factory Preset 16 | CC 44 45 46 47 62 63 75 76; pad-row buttons 107, 108 | Official software (a plugin vendor's profile), matched by a community preset file | SX; not used |

## Open questions, until someone with the hardware checks

1. **Whether the controls send as described right after the messages**, with
   no button pressed, whatever mode the keyboard was left in.
2. **The encoder's relative encoding**, so it can be declared.
3. **The Linux names** of the 25/49/61's third port. The matcher takes both
   the `MIDIIN3` form and the `Mackie/HUI` form.
4. **The fader buttons:** momentary (as in Bitwig) or latching (as in Ableton).

## How to check on hardware

In RackForge, open Controllers with the keyboard connected. The package
attaches to the third port and sends its messages. Then move each control:
the MIDI activity must show the messages above. A difference goes here as a
correction, with the firmware version.
