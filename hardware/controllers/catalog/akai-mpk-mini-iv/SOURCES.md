# Akai Professional MPK mini IV — sources

The package describes the keyboard in its **DAW preset**, read on its **DAW
port**. RackForge puts it in that preset on connect. No value was measured on
hardware.

Of the three kinds of preset, only the DAW preset has a single, known meaning
for the knobs:
- The **Plugin** preset sends them to the Studio Instrument Collection's
  port.
- The **User** presets carry whatever the player saved. Akai does not publish
  their factory contents, and the IV has no downloadable editor.

Akai's own control scripts for the IV come only through the inMusic Software
Center, after registering the hardware, so they were not read. Ableton's
decompiled script lacks the modules that name the controls. **Bitwig's
extension is the only source for the knobs' messages and the preset
message.** The package keeps to what that source states outright, and leaves
the rest out.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK mini IV User Guide v1.2 | https://cdn.inmusicbrands.com/akai/mpk-mini-4/guides/MPK%20mini%20IV%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `ced6bda0bfeccfc669df1cc0861d6e89ef58af3a1c9c401f3050657ef8d17dc8` |
| ALG | MPK mini IV Ableton Live Setup Guide v1.0 | https://cdn.inmusicbrands.com/akai/mpk-mini-4/MPK%20mini%20IV%20-%20Ableton%20Live%20Setup%20Guide%20-%20v1.0.pdf | 2026-09-25 | `0ad5eb89ce301fbe4e4d43e7e798d2ba9a0677d98c9dddbdfb3a2616dd333165` |
| BW | Bitwig, `mpkmk4/`: `MpkMidiProcessor.java`, `MpkHwElements.java`, `controls/Encoder.java`, `MpkMiniMk4ControllerExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/akai/mpkmk4 (commit `a27f2f4b`) | 2026-09-25 | `d6c9b93f…c3554b`, `c33ab194…262702`, `e91bb51c…86a`, `b990d528…774b` |
| AB | Ableton Live 12 `MPK_mini_IV/__init__.py` (decompiled; `midi.py`, `elements.py` absent) | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPK_mini_IV | 2026-09-25 | `0951bfbf…f1c6` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Five ports | MIDI Port (keys, pads, wheels, pedal), DAW Port, Plugin Port, Software Control Port, Din Port | Documented | UG p6 |
| DAW port names | Mac `MPK mini IV DAW Port`; Linux `MPK mini IV MPK mini IV DAW Por`; Windows `MIDIIN2 (MPK mini IV)` / `MIDIOUT2 (MPK mini IV)` | Official software | BW definition |
| The DAW preset | chosen with the PLUGIN/DAW button (lit red) for DAW control scripts | Documented | UG item 9; ALG p3 step 2 |
| Product byte | 0x5D: Identity Reply `F0 7E 7F 06 02 47 5D 00 19 …` | Official software ×2 | BW `DEVICE_RESPONSE_HEADER` with `setHeader(0x5D)`; AB `identity_response_id_bytes = (71, 93, 0, 25)`; AB USB product 93 |
| Select the DAW preset | `F0 47 7F 5D 2D 00 00 F7`, to the DAW port | Official software (one source) | BW `set_preset_daw`, sent first in `startConnection` |
| Knobs | CC 24–31, channel 1, relative in two's complement, on the DAW port | Official software (one source) | BW `new Encoder(i, 0x18 + i, …, getDawMidiIn())`, `createRelative2sComplementCCValueMatcher(0, ccNr, 200)` |
| Endless knobs | yes | Documented | UG item 25 |
| Not sent | the script's other messages: screen ownership, clip mode, pad colours, "pad notes" | — | BW `startConnection`. They serve Bitwig's display and clip launcher; RackForge does not need them |
| Not declared | pads and transport buttons on the DAW port | — | BW binds them, but their messages depend on the script's other modes. The pads play on the MIDI port as they are |
| Slots | knobs `control-1` (no faders) | Convention | catalog README |
| Roles | none | — | Roles do not read relative encoders yet |

## Open questions, until someone with the hardware checks

1. **Everything from BW**: the preset message, and the knobs' CCs and
   encoding.
2. **Whether the DAW preset needs a script's hello first.** Ableton's script
   sends one (`MAIN_MODE_MESSAGE_ID`), but its value is in the missing
   `midi.py`.
3. **Windows port names** without Akai's Windows MIDI driver (UG p6 notes the
   names differ).
