# Akai Professional MPD218 — sources

The package describes the MPD218 on **Preset 1, "chroma10"**. No value was
measured on hardware.

The User Guide sends players to Akai's webpage for the "Preset
Documentation". That page offers the factory presets themselves as files, and
each file is a SysEx preset dump. Ableton's script corroborates the pads. No
second source states the knobs.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Akai, "MPD218 Factory Presets": `Preset1-chroma10.mpd218` (and Presets 2–9) | https://cdn.inmusicbrands.com/akai/MPD218-FactoryPresets.zip | 2026-09-25 | `6119dcfb3946f6e9a24aaf05b1b40f0e38403f2d37c28999327b43e6586959e6` (Preset 1); zip `ef623408…4b9e598` |
| UG | MPD218 User Guide v1.0 | https://cdn.inmusicbrands.com/akai/attachments/MPD218/MPD218-UserGuide-v1.0.pdf | 2026-09-25 | `72bf5f3ae6d3f2266b07875ba175f7b5da3d1653f6f5dc00c6b7d568d3be608b` |
| AB | Ableton Live 12 `MPD218/MPD218.py`, over `_MPDMkIIBase` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/MPD218 (commit `e83d5192`) | 2026-09-25 | `bd34c117…bd67c2` |

## The preset file

`F0 47 00 34 10 04 1D 01`, then the name (8 bytes), five setting bytes, 48
pad records and 18 knob records, then `F7`. 0x34 is the MPD218's product
byte (AB USB product 52).

- **A pad record** is 8 bytes, and begins with the MIDI channel counted from
  1 (0x0A, channel 10) and the note. In Preset 1 the notes run 36–83, pad by
  pad and bank by bank. In Presets 2, 4 and 6 the channel is 0x01, which
  confirms this is the channel byte.
- **A knob record** holds the channel from 1 (0x01), the CC, the minimum 0
  and the maximum 0x7F, then zero bytes. The CCs are 3, 9, 12–27. They are
  the same in all nine presets.

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Port name | `MPD218` | Official software | AB `model_name="MPD218"` |
| Product byte, for the record | 0x34 (52) | Official software ×2 | PR header; AB `product_ids=[52]` |
| Knobs | six 360° knobs, three control banks | Documented | UG items 3–4 |
| Knob CCs | bank A 3, 9, 12, 13, 14, 15; bank B 16–21; bank C 22–27; channel 1; 0–127 | Official software (the maker's preset file, one source) | PR |
| Knob type | absolute, read from the record's zero type byte | Official software, reading assumed | PR. The format is not documented; see the open questions |
| Pads | 16 pads, three pad banks; notes on channel 10: A 36–51, B 52–67, C 68–83 | Documented (count); Official software ×2 for bank A (PR; AB), one for B and C (PR) | UG item 6; PR; AB `PAD_IDS`, `PAD_CHANNEL = 9` |
| Pad order | pad 1 bottom left; bottom row 36–39, top row 48–51 | Official software | AB `PAD_IDS`, top row first |
| Slots | knob banks A, B, C on control rows 1, 2, 3; bank A's pads by note; banks B and C none | Convention | catalog README |
| Roles | the four-knob rule on bank A: cutoff, resonance, filter envelope amount, LFO rate | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The knobs' type.** The preset format is undocumented, so absolute is a
   reading of a zero byte. Inc/Dec would need `encoder` inputs.
2. **Which preset the MPD218 starts on** (Preset 1 is assumed).
3. **The Linux and Mac port names** (the endpoint matches "MPD218" anywhere
   in the name).
