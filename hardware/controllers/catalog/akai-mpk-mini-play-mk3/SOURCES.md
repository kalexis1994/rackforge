# Akai Professional MPK mini Play mk3 — sources

The package describes the keyboard on **Favorite 1**. No value was measured
on hardware.

The User Guide names the controls but gives no MIDI values. The values come
from Akai's factory favorites, shipped with its MPK mini Play mk3 Favorite
Editor. The editor was downloaded from Akai's page with the user's leave,
and extracted, not installed or run.

The files have the MPK mini mk3's record structure. Their fields follow the
same order, with one field more before the tempo (see
`../akai-mpk-mini-mk3/SOURCES.md`, where Bitwig's program checks the
reading).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | MPK mini Play mk3 User Guide v1.0 | https://cdn.inmusicbrands.com/akai/mpk-mini-play-mk3/MPK_mini_Play_mk3_User_Guide_v1.0.pdf | 2026-09-25 | `bc24e9e8dcc5851b6f46a5bbac9463518b38fc5f4935bddc0265f5aae3cd53d0` |
| FAV | Akai, `Fav1.mpkminiplaymk3` (and Fav2–8), inside `AkaiMPKminiPlaymk3FavoriteEditorOSX 1.0.3.dmg` → `/Library/Application Support/inMusic/MPKminiPlaymk3FavoriteEditor/…` | https://cdn.inmusicbrands.com/akai/MACEDITORS/AkaiMPKminiPlaymk3FavoriteEditorOSX%201.0.3.dmg | 2026-09-25 | `feef8594e6aab4625e72b32054efa81ab0fc66fcca2201954c5cfc594a9124f8` (Fav 1); DMG `825aa00d226cd6cb5e5cdb0d63b0b442b7ac2f3972545682e95fce4ef4c62db5` |
| EG | MPK mini Play mk3 Editor User Guide v1.0 (in the app bundle) | as above | 2026-09-25 | `139c16f4fd076c54f588f9f62dfae64aeab7f5746ce9e16e3a80407777e9f689` |

## The favorite file

| Field | Meaning | Favorite 1 |
|---|---|---|
| 1 | pad channel (0-based) | 9 |
| 2 | pad aftertouch | 2 |
| 3 | keybed & controls channel (0-based) | 0 |
| 15–17 | joystick X: mode, CC, CC | 0 0 0 (Pitchbend) |
| 18–20 | joystick Y: mode, CC, CC | 2 1 1 (Dual CC, CC 1 both ways) |
| 21–36 | 16 pad notes, bank A then bank B | 36 … 51 |
| 37–68 | 8 knobs × (CC, Min, Max, name) | 70 0 127 "Q-Link" … 77 0 127 |
| 69–78 | the internal sound and its effects | — |

Across the eight favorites:
- The channels and the joystick are the same in all of them.
- The knobs are CC 70–77 in seven, and CC 1–8 in Favorite 2.
- The pads are 36–51 in five of them. Favorite 4 has the MPC layout; 7 and 8 lay the pads out in a scale.

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Port name | `MPK mini Play mk3` | Official software (Akai's editor, the name it looks for) | the editor binary's device string |
| Knobs | four 270° knobs, two banks: bank A CC 70–73, bank B CC 74–77, 0–127, channel 1 | Documented (knobs, banks, which four per bank); Official software (values) | UG items 16–20; EG p5 ("4 … when Bank A is active, and the other 4 … Bank B"), p11; FAV |
| The knobs with the internal sounds | bank A sets filter, resonance, reverb, chorus; bank B attack, release, EQ low, EQ high; "in USB mode" they send their CCs | Documented | UG items 17–20. Whether they also send the CCs with the internal sounds on is not said |
| Pads | notes on channel 10: bank A 36–43, bank B 44–51 | Official software | FAV; EG p5 (8 pads per bank) |
| Joystick | X pitch bend; Y CC 1 up and down, 0 at centre | Official software; Documented (it sends pitch bend or CCs) | FAV; UG item 3 |
| Sustain | the input; CC 64 | Documented (input); Convention (the MIDI standard) | UG rear panel item 4 |
| Internal Sounds button | off: "send and receive MIDI only using the USB port" | Documented | UG item 14 |
| Slots | knobs 1–8 (bank A then B) `control-1`; pads by note; the joystick's Y `mod-wheel` | Convention | catalog README |
| Roles | the eight-knobs rule: tone on bank A, the amp envelope on bank B | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Which favorite the keyboard starts on.** The package describes Favorite
   1. Favorites 2, 4, 7 and 8 send other knob CCs or pad notes.
2. **Whether the knobs send their CCs while the internal sounds are on.**
3. **The Linux port name** (`… MIDI 1`?). The endpoint matches the model's
   name wherever it appears.
