# M-Audio Oxygen Pro Mini — sources

The package describes the keyboard in the **DAW mode RackForge puts it in
when it connects**, as for the Oxygen Pro (`../m-audio-oxygen-pro/SOURCES.md`,
whose caveats apply here). No value was measured on hardware.

The Preset mode is also avoided for a reason of its own. The Mini's factory
preset sends its knobs as CC 120–127, which are Channel Mode messages:
Reset All Controllers, All Notes Off, Omni and Mono/Poly. They could not be
passed to an instrument as they are.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| MUG | Oxygen Pro Mini User Guide v1.2 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Oxygen%20Pro%20Mini%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-24 | `97e4678b2fc989aaf2782585af1f7f2faff1007708ba8341e4314e773e9e2dd1` |
| MAB | Ableton Live 12 MIDI Remote Script `Oxygen_Pro_Mini` (decompiled): `__init__.py`, `oxygen_pro_mini.py`, over the `Oxygen_Pro` script | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Oxygen_Pro_Mini | 2026-09-24 | `5ba4ce56…e630e7` (\_\_init\_\_.py), `dc95ceec…d5fca9` (oxygen_pro_mini.py) |
| MBW | Bitwig, `OxygenProMiniExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/maudio/oxygenpro | 2026-09-24 | `34c28a4e…84bed1f8a` |
| BL | Baeldung, "Using a MIDI Keyboard on Linux" (`aconnect` listing of an Oxygen Pro Mini) | https://www.baeldung.com/linux/midi-keyboard-operation | 2026-09-24 | — (web page) |
| SX | Spectrasonics, Omnisphere setup for the Oxygen Pro Mini (factory preset) | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-oxygen-pro-mini | 2026-09-24 | — (web page) |
| — | the Oxygen Pro's AB and BW | see `../m-audio-oxygen-pro/SOURCES.md` | | |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The Mini runs the Oxygen Pro script, four controls wide | `Oxygen_Pro_Mini(Oxygen_Pro)`, `session_width = 4`; Bitwig `OxyConfig(4, true, false, false)`: faders, no master, no scene buttons | Official software ×2 | MAB; MBW |
| Mode messages | the Oxygen Pro's 6D 02, 6E 02, 6E 07 (and 6B 01, 6C 03, left out) | Official software ×2 | AB `on_identified` (inherited); BW `MidiProcessor` |
| Faders 1–4 | CC 12–15, channel 1 | Official software ×2 | AB `Fader_n = 12 + n` over 4; BW `SLIDER_1 = 0x0C` |
| Knobs 1–4 | CC 22–25, channel 1, absolute | Official software ×2 | AB; BW `KNOB_1 = 0x16` |
| Function buttons 1–4 | CC 32–35, 127 / 0 | Official software (BW, momentary; AB non-momentary, as for the Oxygen Pro) | They take no slot |
| Pads | notes 40 41 42 43 48 49 50 51, channel 1 | Official software ×2 | MAB `pad_ids`; BW the first eight of `PAD_NOTE_NR` |
| The Pad Bank button switches the pads between two banks | what bank 2 sends in this mode is **not documented** | — | MUG p10. The package declares bank 1 only |
| Transport, bank, back | Loop 114, << 115, >> 116, Stop 117, Play 118, Record 119, Bank < 110, Bank > 111, Back 104 | Documented (the buttons); Official software ×2 (the CCs) | MUG p9; AB `elements.py`; BW `OxygenCcAssignments` |
| No Metronome button | the Mini's is TEMPO | Documented | MUG p10; not declared |
| Ports | Windows `MIDIIN3 (Oxygen Pro Mini)` / `MIDIOUT3 (…)`; Mac `Oxygen Pro Mini Mackie/HUI` | Official software | BW `OxygenProExtensionDefinition` with keys "Mini" |
| Linux port names | `Oxygen Pro Mini USB MIDI`, `… MIDI DIN`, `… Mackie/HUI`, `… Editor` | Community | BL (`aconnect -i` output) |
| USB product | 0x103B (4155), M-Audio 0x0763 | Official software | MAB `product_ids=[4155]` |
| Preset mode knobs, for the record | CC 120–127 (bank 1: 120–123, bank 2: 124–127) | Official software (a plugin vendor's profile) and Community (a third-party script), which agree | SX; not used |
| Roles | knobs: cutoff, resonance, filter envelope amount, LFO rate; faders: attack, decay, sustain, release | Convention: RackForge's four-knob, four-fader rule (catalog README) | — |

## Open questions, until someone with the hardware checks

1. Everything the Oxygen Pro's file lists. The Mini's firmware answering the
   messages is what both scripts rely on.
2. **The pads' second bank** in this mode: which notes it sends.
