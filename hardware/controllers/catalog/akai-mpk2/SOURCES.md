# Akai Professional MPK2 Series (MPK225, MPK249, MPK261) — sources

The packages describe each keyboard on **Preset 1, "LiveLite"**, the preset
Akai has players choose for Ableton Live ([KB]). No value was measured on
hardware.

The User Guides describe every parameter a preset holds, but give no preset's
values, and the MPK2 has no editor to download. Two scripts read these
keyboards:
- **Ableton's script** reads this preset. It is the **only source for the
  preset's messages**. It agrees across the three models, value for value.
- **Akai's own Bitwig scripts** read another preset, "Bitwig", and give the
  port names.

The CCs are the same scheme inMusic uses for the M-Audio Oxygen Pro in DAW
mode (see `../m-audio-oxygen-pro/SOURCES.md`). That is corroboration by
convention only.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK225, MPK249, MPK261 User Guides v1.0 | https://cdn.inmusicbrands.com/akai/attachments/MPK249/MPK249%20-%20User%20Guide%20-%20v1.0.pdf (and MPK225, MPK261 alike) | 2026-09-25 | 225 `974884e6…c438e377`, 249 `e29c6244…6d119e96`, 261 `45b54447…edf24e3` |
| AB | Ableton Live 12 `MPK225/MPK225.py`, `MPK249/MPK249.py`, `MPK261/MPK261.py` and their `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts (commit `e83d5192`) | 2026-09-25 | `37dfadbf…cd99178`, `2770161a…0895b1`, `8f873f2b…c997959f` |
| AKB | Akai, "MPK2 Series Bitwig Scripts v1.0.8": `MPK225/249/261.control.js`, `MPK2_common.js`, and the "MPD2 Series, MPK2 Series Bitwig Studio Program Documentation v1.0" | https://cdn.inmusicbrands.com/akai/attachments/MPK249/MPK2_Series_Bitwig_Scripts_v1.0.8.zip | 2026-09-25 | zip `620e1128…152119b`; `MPK2_common.js` `2e8a5801…18665cf` |
| KB | Akai, "Akai MPK2 Series — Setup in Ableton Live" | https://support.akaipro.com/en/support/solutions/articles/69000814185 | 2026-09-25 | — (web page) |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| The preset | Preset 1, "LiveLite"; "the presets ... may vary depending on the specific model" | Documented | KB |
| Port names | Port A: Windows `MPK2xx`, Mac `MPK2xx Port A`, Linux `MPK2xx MIDI 1`; Remote: `MIDIIN4 (MPK2xx)`, `MPK2xx Remote`, `MPK2xx MIDI 4` | Official software (Akai's) | AKB `addDeviceNameBasedDiscoveryPair` in each `.control.js` |
| Other ports | USB A and USB B channels; a 5-pin MIDI port | Documented | UG "MIDI Channel: Common, USB A1–A16, USB B1–B16" |
| Product bytes, for the record | 225 0x23, 249 0x24, 261 0x25 (SysEx `F0 47 00 <id>`; USB product 35, 36, 37) | Official software ×2 | AKB `PRODUCT_ID`; AB `product_ids` |
| Knobs (bank A) | CC 22–29, absolute, channel 1 | Official software (one source) | AB `Encoders`, `make_encoder` → `MapMode.absolute` |
| Faders (bank A; 249, 261) | CC 12–19, channel 1 | Official software (one source) | AB `Sliders` |
| Switches (bank A; 249, 261) | CC 32–39, channel 1, momentary | Official software (one source) | AB `Arm_Buttons`, `make_button` (momentary) |
| MPK225 switches | four; **what they send in this preset is not known** | Documented (count) | UG item 12 and item 14 ("36 independent parameters with the knobs and switches"). Not declared |
| Control banks | three, A–C | Documented | UG item 14/15. Only bank A is declared: AB reads no other |
| Pads (bank A) | notes on channel 2: 60 62 64 65 / 67 69 71 72 (225); up to 86 (249, 261), bottom row first | Official software (one source) | AB `Drum_Pads`, channel 1 zero-based, rows listed top first |
| Pad banks | four | Documented | UG. Only bank A is declared |
| Pad slots | the slot of the pad in the same place on a 36–51 grid | Convention | catalog README; these pads send other notes |
| Pitch-bend wheel, modulation wheel | pitch bend; CC 1 | Documented | UG items 3–4 |
| Sustain pedal | CC 64 | Convention (the MIDI standard) | UG rear panel names the input; the preset's Footswitch Type includes Sustain |
| Transport | Loop 114, Rewind 115, Fast-forward 116, Stop 117, Play 118, Record 119, channel 1 | Official software (one source); Documented (buttons, and the MIDI CC option) | AB `add_button`; UG item 25/28 |
| Slots | faders `control-1`, knobs `control-2` (249, 261); knobs `control-1` (225); pads as above; modulation wheel; Rewind and Fast-forward `step` | Convention | catalog README |
| Roles | 249/261: the eight-knobs-eight-faders rule without a master fader; 225: the eight-knobs rule; Play and Stop take the transport | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Everything the preset sends** rests on Ableton's scripts alone.
2. **Which preset the keyboard starts on.** Preset 1 is LiveLite, but whether
   the keyboard powers up on the last preset used is not documented.
3. **Control banks B and C, pad banks B–D, and the MPK225's switches.**
4. **The Mac port names of Port B and the 5-pin MIDI port.** The Mac
   endpoint takes "Port A" alone, so they are never claimed whatever they
   are called.
