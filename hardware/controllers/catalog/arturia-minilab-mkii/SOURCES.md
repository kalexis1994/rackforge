# Arturia MiniLab mkII — sources

The package describes the MiniLab mkII **in memory 1 ("Analog Lab")**, the
memory it loads at power-up ([UM] p16) and which the player cannot edit
([UM] p26). No value was measured on hardware.

**Arturia publishes no table of memory 1's messages.** Its manual gives the
pad notes and says the encoders are absolute by default for Analog Lab; the
numbers come from community sources that read a factory unit and agree. They
are marked as such. The package sends nothing to the keyboard.

The official scripts do not describe memory 1. Ableton's reprograms every
control by SysEx and saves that to the player's memory 8; Cubase's loads
memory 2 with its own settings. Neither is used for a value here.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| UM | MiniLab mkII User Manual 1.1 | https://dl.arturia.net/products/minilab-mkII/manual/minilab-mkii_Manual_1_1_EN.pdf | 2026-09-25 | `32117bbeaea73bc351bd844ef3c54031bcc0305edb414ee85d87d23b56ae1389` |
| AR | Ardour, `share/midi_maps/Arturia_MiniLab_mkII.map` ("Works with device's factory settings, 1 (Analog Lab)") | github.com/Ardour/ardour | 2026-09-25 | commit `f94eab98` (Community) |
| JK | jdw-keys-backend, `src/midi_mapping.rs` L51–190 | github.com/estrandv/jdw-keys-backend | 2026-09-25 | commit `14c23da9` (Community) |
| BS | BespokeSynth, `resource/userdata_original/controllers/Arturia MiniLab mkII MIDI 1.json` and `…_PADS.json`; PRs #399, #408 | github.com/BespokeSynth/BespokeSynth | 2026-09-25 | commits `2f4f6936`, `9795bfe3` (Community) |
| SC | super-controller, `src/shared/drivers/arturia-minilab-mkii.ts` L85, L135–213 | github.com/aolsenjazz/super-controller | 2026-09-25 | commit `dd887c56` (Community) |
| NB | minilab-mkII-bitwig, `minilab-mkII.control.js` L5, L79–83 | github.com/Nettsu/minilab-mkII-bitwig | 2026-09-25 | commit `82c249ed` (Community) |
| AS | Alpha-Sequencer, `Source/ArturiaMiniLabMk2Profile.cpp` L84–150 | github.com/Alphapolygon/Alpha-Sequencer | 2026-09-25 | commit `51c48e40` (Community) |
| MF | bitwig-arturia-minilab-mkii, `MiniLab mkII.control.js` L7–17 (port names per OS), L55, L104 | github.com/morris-frank/bitwig-arturia-minilab-mkii | 2026-09-25 | commit `0b31a09c` (Community) |
| CU | Steinberg's Cubase MIDI Remote script for the MiniLab mkII, read in a copy: `references/cubase/arturia/minilab_mk2/arturia_minilab_mk2.js` L5–14 | github.com/PetersDigital/OpenMIDIControl | 2026-09-25 | commit `e126e3e2` (Official software, via a third party) |
| AB | Ableton Live 12 `MiniLab_mkII` and `MiniLab` scripts | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `0336151d3ad8c9c8213ae327d51f8e9381cd18aa` |

## Facts, one by one

Channels from 1. **Community ×N** counts independent sources; copies count
once.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Memory 1 at power-up, not editable | — | Documented | UM p16, p26 |
| Encoders absolute by default for Analog Lab; "Relative 1" is binary offset around 64 | — | Documented | UM p38 |
| Encoders 2–8 | CC 74, 71, 76, 77, 93, 73, 75, channel 1, absolute | Community ×5 | AR, JK, BS, SC, AS |
| Encoders 10–16 | CC 18, 19, 16, 17, 91, 79, 72, channel 1, absolute | Community ×5 | same |
| Encoders 1 and 9 (the two that click) | CC 112 and 114, channel 1, relative binary offset (61–63 down, 65–67 up, a 64 after each step) | Community ×5 | JK, BS (PR #408), SC, NB, AS |
| Their clicks | CC 113 and 115, 127 pressed, 0 released | Community ×4 | JK, BS, NB, MF |
| Pads 1–8 | notes 36–43 | Documented | UM p19 |
| Their channel | 10 | Community ×4 | AR, BS (PR #399 "out-of-the-box … channel 10"), MF, JK |
| Pads 9–16 | CC 22–29, channel 1, 127 / 0 | Community ×3 | AR (23–29 for pads 10–16), JK, BS |
| Pitch strip | pitch bend, channel 1 | Community ×3 | JK, BS, SC |
| Modulation strip | CC 1, channel 1 | Community ×4 | JK, BS, SC, AS |
| Keys | channel 1 | Community ×3 | AR, JK, MF |
| Port name | `Arturia MiniLab mkII` (Windows, Mac); `Arturia MiniLab mkII:Arturia MiniLab mkII MIDI 1 20:0` (Linux); one port | Official software (the name); Community ×4 (Linux) | AB `__init__.py` model_name; MF; rdbende/MIDI-monitor `636fc874`; k1ln/midireef `0dad12b3`; goodfaiter/arturia_ledfx `47e31c93` |
| Identity Reply, for the record | `F0 7E id 06 02 00 20 6B 02 00 04 02 …` | Official software (via a copy); Community ×2 | CU `expectSysexIdentityResponse('00206B','0200','0402')`; mfeyx/bitwig-arturia-minilab-mkii `26fb8ce1`; molenick/midilab `6422b86e`. Not used: the name singles the product out |
| Not used | Shift + encoders 1 and 9 (CC 7 and 116 in one source, not confirmed for memory 1); the sustain pedal's CC; Shift and Oct −/+ (SysEx) | Community ×1 or not found | — |
| Flagged, not used | presets and notes taken from Arturia's MIDI Control Center | — | its licence forbids reverse engineering |
| Slots | encoders 1–8 `control-1`, 9–16 `control-2` (no faders); pads 1–8 by note (`switch-1`); pads 9–16 none (CC); modulation strip `mod-wheel` | Convention | catalog README |
| Roles | encoders 2–8 as the rule for eight knobs and no faders; encoder 1 none (relative) | Convention | catalog README |
| Pads 9–16 declared as buttons | a pad cannot send a CC in the package format | Convention | — |

## Open questions, until someone with the hardware checks

1. Whether **pads 1–8 send poly aftertouch** in memory 1.
2. The **sustain pedal's** CC in memory 1.
3. Whether the **pitch strip** returns to zero or holds.

## How to check on hardware

Recall memory 1 (or power the MiniLab mkII up), open Controllers in
RackForge, choose it and press **Check controls**. Turn every encoder both
ways, click 1 and 9, hit both pad banks and touch both strips; the report
lists what arrived against the rows above. A difference goes here as a
correction, with the firmware version.
