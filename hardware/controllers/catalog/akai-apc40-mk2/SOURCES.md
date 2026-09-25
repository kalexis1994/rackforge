# Akai Professional APC40 mkII — sources

The package describes the APC40 mkII in **Generic Mode**, the mode it starts
in ([CP] "The unit defaults to Mode 0 on startup"). RackForge sends it
nothing. No value was measured on hardware.

The Ableton Live modes would bank nothing by channel. Two things rule them
out here:
- Entering them takes an Introduction message carrying the device's own
  SysEx ID, which a package cannot know ahead.
- Ableton follows it with a "dongle" challenge that is not documented.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| CP | APC40 Mk2 Communications Protocol v1.2 | https://cdn.inmusicbrands.com/akai/attachments/apc40II/APC40Mk2_Communications_Protocol_v1.2.pdf | 2026-09-25 | `3af85d0b338a68581360633f38db9267119a34d3e9f7ffaf68be24658ce932b1` |
| AB | Ableton Live 12 `APC40_MkII/APC40_MkII.py`, `__init__.py`, over `_APC/ControlElementUtils.py` and `APC.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts (commit `e83d5192`) | 2026-09-25 | `6be46664…4614e9` (APC40_MkII.py) |
| BW | Bitwig, `apc40_mkii/APC40MKIIControllerExtensionDefinition.java` | https://github.com/bitwig/bitwig-extensions (commit `a27f2f4b`) | 2026-09-25 | `df9b8584…fa14c17` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Startup mode | Generic Mode (0x40); Ableton Live Mode 0x41 and Alternate 0x42 are chosen by the host's Introduction message | Documented | CP Outbound Message Type 0 |
| Port name | `APC40 mkII`; Linux `APC40 mkII MIDI 1`; also `Akai APC40 MkII` | Official software ×2 | BW; AB `model_name` |
| Product byte, for the record | 0x29 (41) | Documented; Official software | CP; AB `product_ids=[41]`, `_product_model_id_byte` |
| Track faders 1–8 | CC 7 on channels 1–8 | Documented; Official software | CP (channel = track); AB `make_slider(track, 7)` |
| Master fader, crossfader | CC 14, CC 15, channel 1 | Official software | AB |
| Track Control knobs 1–8 | CC 48–55, channel 1, absolute, never banked | Documented; Official software | CP Generic Mode notes; AB `make_ring_encoder(48 + track)`, `MapMode.absolute` |
| Device Control knobs 1–8 | CC 16–23, absolute; in Generic Mode on the selected track's channel (1–8, master 9) | Documented; Official software | CP Generic Mode notes; AB `make_ring_encoder(16 + index)` |
| Cue level, Tempo | CC 47, CC 13, relative two's complement | Official software | AB `make_encoder`, `relative_two_compliment` |
| Clip grid | notes 0–39, channel 1; top row 32–39, bottom row 0–7 | Documented; Official software | CP Clip Launch 1–40; AB `32 + track - 8 * scene` |
| Per-track buttons | Record Arm 48, Solo 49, Activator 50, Track Selection 51, Clip Stop 52, Crossfader A/B 66, each on its track's channel | Documented; Official software | CP note table; AB |
| Track Selection in Generic Mode | sends nothing; picks the Device Control channel | Documented | CP. Left out |
| Scene Launch 1–5, Stop All Clips, Master | notes 82–86, 81, 80 | Documented; Official software | CP; AB |
| Device buttons 1–8 | notes 58–65 | Documented; Official software | CP; AB |
| Pan, Sends, User, Metronome, Play, Record, Up, Down, Right, Left, Shift, Tap Tempo, Nudge −, Nudge +, Session Record, Bank | notes 87, 88, 89, 90, 91, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103 | Documented; Official software | CP; AB |
| Stop | note 92 | Documented | CP. AB maps none |
| Foot pedal | CC 64, channel 1 | Official software | AB `make_pedal_button(64)` (a CC) |
| Button behaviour in Generic Mode | several buttons toggle their LED; the protocol reports every button as note-on when pressed and note-off when released | Documented | CP |
| Held from instruments | every control (`plays = false`): no keys; its grid and buttons send notes | Convention | catalog README (added 2026-09-25) |
| Slots | faders `control-1`, Track Control `control-2`, Device Control `control-3`; the grid's two bottom rows the switches; Left/Right `step`, Down/Up `step-2`; Metronome `switch-3.3` | Convention | catalog README |
| Fn button | Shift | Official software (the device's modifier) | AB |
| Actions | Play `transport_play`, Stop `transport_stop`; their notes are held back from the instruments | Convention | catalog README (Play and Stop take the transport) |
| Roles | faders and Track Control knobs as the first rule; the master fader the master level | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Which track is selected at startup**, which decides the Device Control
   knobs' channel. The package declares them on channel 1, track 1's.
2. **Whether a toggling button in Generic Mode reports both its press and its
   release**, as the protocol's inbound section says.
