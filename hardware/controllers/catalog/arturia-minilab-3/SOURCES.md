# Arturia MiniLab 3 — sources

The package describes the MiniLab 3 in its **ARTURIA program**, or in a
User program that keeps the default template. Arturia's chart gives the two
the same numbers ([CH]). No value was measured on hardware.

Every number in the package is Arturia's own, from:
- the User Manual;
- the MIDI Control Center manual;
- the MIDI implementation chart in Arturia's FAQ.

Bitwig's extension and Ableton's script corroborate the pads, the transport
and the port names. They read the DAW program, whose knobs and faders send
other numbers (below).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UM | MiniLab 3 User Manual 1.0.5 | https://dl.arturia.net/products/minilab-3/manual/minilab-3_Manual_1_0_5_EN.pdf | 2026-09-25 | `5d13046f14b60dff4fd46fe7697e99170b61f9f059a0aed20f0c0e8315e31d76` |
| MCC | MIDI Control Center for MiniLab 3, User Manual 1.14.1 | https://dl.arturia.net/products/minilab-3/manual/minilab-3-mcc_Manual_1_14_1_EN.pdf | 2026-09-25 | `27ead57bbc683b3accbd62374c16783c8da2c441cfd544e00ef9b52a85befe7e` |
| CH | Arturia FAQ, "MiniLab 3 - General Questions": "MIDI PORTS" and "MIDI IMPLEMENTATION CHART" | https://support.arturia.com/hc/en-us/articles/6189475866396-MiniLab-3-General-Questions | 2026-09-25 | — (web page) |
| BW | Bitwig, `MiniLab3Extension.java`, `MiniLab3ExtensionDefinition.java`, `MinilabRgbButton.java` | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/arturia/minilab3 (commit `3379f3d4`) | 2026-09-25 | `bbe692e3…35d903`, `92292adc…6ac00c`, `b2f1ba36…d65563` |
| AB | Ableton Live 12 MIDI Remote Script `MiniLab_3` (decompiled): `__init__.py`, `elements.py`, `midi.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MiniLab_3 (commit `0336151d`) | 2026-09-25 | `1aacc8a5…84f143`, `56dea665…04b13d`, `f966ecbd…434530` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Programs | ARTURIA, DAW, and up to five User programs; Shift + Pad 3 cycles them | Documented | UM 4.4.2; MCC 4.2.9 |
| Ports | MIDI, DIN Thru, MCU/HUI, ALV; "Minilab3 MIDI" is the one to play from | Documented | UM 5.2; CH "MIDI PORTS" and "select Minilab3 MIDI" |
| Port names | Windows `Minilab3` (`Minilab3 MIDI` with the MIDI Control Center; `N- Minilab3 MIDI` for more units); Mac `Minilab3 MIDI`; Linux `Minilab3 Minilab3 MIDI` | Official software | BW `listAutoDetectionMidiPortNames` |
| The MiniLab 37 | `Minilab37` | Official software | BW `MiniLab37ExtensionDefinition` |
| USB ids and Identity Reply, for the record | Arturia 7285 (0x1C75), product 8715; reply `00 20 6B 02 00 04` | Official software | AB `controller_id`, `identity_response_id_bytes`. Not needed: the port name singles the model out |
| Knobs | CC 74, 71, 76, 77, 93, 18, 19, 16, keyboard channel | Documented ×2 | UM 5.4.1–5.4.2; CH "Arturia" and "Users" columns |
| Knob mode | absolute: CC between Min and Max, or NRPN; no relative setting | Documented | MCC 3.6 |
| Faders | CC 82, 83, 85, 17, keyboard channel | Documented ×2 | UM 5.4.3; CH |
| DAW program, for the record | knobs CC 86, 87, 89, 90, 110, 111, 116, 117; faders 14, 15, 30, 31; Shift 27; main encoder 28 | Documented; Official software ×2 | CH "DAWs" column; AB `Elements`; BW `ENCODER_CC_MAPPING`, `SLIDER_CC_MAPPING`. Not declared: the package describes ARTURIA |
| Pads | notes on channel 10: bank A 36–43, bank B 44–51 | Documented; Official software ×2 | UM 4.4, 5.4.4 table; AB `Pad_Bank_A`/`B`, channel 9 zero-based; BW `0x24 + i`, `0x2C + i` on channel 9 |
| Transport | Shift + Pad 4–8 (or the Transport pad bank): Loop 105, Stop 106, Play 107, Record 108, Tap Tempo 109; channel 1; 127 on press, 0 on release | Documented (functions, CCs); Official software ×2 (CCs, values) | UM 4.1, 4.4.1; CH "Pad 4"–"Pad 8"; AB `Loop_Button` … `Tap_Tempo_Button`; BW `MinilabRgbButton(…, CC, 105…109, 0)`, `createCCActionMatcher(…, 127)` / `(…, 0)` |
| Touch strips | pitch bend; modulation CC 1 | Documented | UM 4.3 |
| Pedal | the Sustain pedal type sends CC 64 on the keyboard channel | Documented | MCC 4.4, 4.4.1. Which type is factory is not said |
| Keyboard channel | the Default Keyboard Channel; the controls above follow it | Documented | MCC 4.2.2, 3.x "Keyboard"; UM 4.1 (Shift + keys) |
| Not declared | Shift (CC 9), the main encoder (CC 114/112, click 115/113) | Documented (CH) | They drive the keyboard's own menus |
| Slots | faders `control-1.1`–`1.4`; knobs `control-2`; pads by note; modulation strip `mod-wheel` | Convention | catalog README; as the Oxygen Pro Mini |
| Roles | faders attack, decay, sustain, release; knobs cutoff, resonance, filter envelope amount, LFO rate, LFO depth, filter LFO amount, key tracking, amplifier level; Play and Stop take the transport | Convention | catalog README, "Eight knobs and four faders" |

## Open questions, until someone with the hardware checks

1. **Which program the MiniLab 3 starts in.** The manual does not say. In
   the DAW program the knobs and faders send the DAW numbers above, so
   they would not reach the knobs' slots.
2. **The Linux names of the other ports.** The endpoint keeps out "thru",
   "mcu", "hui" and "alv", the words Arturia gives them.
3. **The factory pedal type.**
