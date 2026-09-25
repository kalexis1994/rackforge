# Alesis V25, V49, V61 — sources

The package describes the **first-generation** V25, V49 and V61 in their
factory settings. No value was measured on hardware for this package.

Alesis's user guides document only the wheels; every guide sends the player
to the V Editor for the rest. The knobs come from official software and a
software maker's setup guide; the buttons, pads and channels from community
sources, marked as such. The package sends nothing to the keyboard.

The **V MKII** is not described: only its knobs (CC 74–77) were found, from
a single source, and nothing about its pads.

Alesis's editor was not opened: its licence forbids reverse engineering.
One source, tmick0's alesisvsysex, learnt the editor's SysEx protocol by
sniffing its USB traffic and reads the settings back from the keyboard; its
values are the device's own reply. Another, bliepp, ships configurations
saved from the editor through normal use.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| UG25 | V25 User Guide v1.2 | https://www.alesis.com/rscdn/1074/documents/V25%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `6c91cf11f5e8224ec9c943c30e117edf8e535f02e4ec733e640aeca0029b6acd` |
| UG49 | V49 User Guide v1.2 | https://www.alesis.com/rscdn/1075/documents/V49%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `57ec0386904ede58d087a522a9612811f5ee5ab2c39ecb659c7b2006c0bee8cb` |
| UG61 | V61 User Guide v1.2 | https://www.alesis.com/rscdn/1076/documents/V61%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `ea0cd572a0aebe81db3b5861d7c91c3320cfc14026a1031d6129908282835bcf` |
| AB | Ableton Live 12 `Alesis_V` script (`__init__.py` L12–18, `Alesis_V.py` L22–23) | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `e83d5192` |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the V25 / V49 / V61 mk1" ("factory default settings") | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-v49-mk1 (and the v25 and v61 pages) | 2026-09-25 | — |
| TM | alesisvsysex, `protocol/model.py` L32–100 (the settings read back from a unit); blog https://lo.calho.st/posts/reverse-engineering-sysex/ | github.com/tmick0/alesisvsysex | 2026-09-25 | commit `3ebd36b7` (Community) |
| BL | Alesis V Series for FL Studio, README L10, `device_Alesis_V_Series.py` L11–15, L146–151 | github.com/bliepp/Alesis-V-Series-for-FL-Studio | 2026-09-25 | commit `b086b433` (Community) |
| GH | Pure Data V49 interface, `V49-interface-nogui/V49-interface-nogui.pd` L53, L62, L74 | github.com/gilbertohasnofb/pd-abstractions-and-libraries | 2026-09-25 | commit `23b97803` (Community) |
| WB | studio-live, `lib/rack/Alesis_V25.mjs` | github.com/wbhb/studio-live | 2026-09-25 | commit `97e9eaa5` (Community) |
| PN | ALSA and Windows port listings: turcofran/omfootctrl `patchbay_jack.xml` L25–26 (`dcb64406`); zynthian-data `Default.ttl` L66, L77 (`4920bea5`); TerraByte-Dev/Keys `docs/HARDWARE.md` L17–22 (`c361b29a`) | GitHub | 2026-09-25 | (Community) |

## Facts, one by one

Channels from 1.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Every size has the same controls | 4 knobs, 4 CC buttons, 8 pads | Documented | UG25, UG49, UG61 |
| Pitch wheel; modulation wheel | pitch bend; CC 1 | Documented | UG p4 (each) |
| Knobs 1–4 | CC 20–23, channel 1, absolute | Official software; SX; Community ×4 | AB `Alesis_V.py` L22–23; SX ("factory default settings"); TM, BL, GH, WB |
| Buttons 1–4 | CC 48–51, channel 1, toggling 127 / 0 | Community ×3 | TM (mode 00 = toggle); BL (README: its own files set them to momentary "instead of toggle"); GH |
| Pads | notes 49, 41, 42, 46, 36, 37, 38, 39, channel 10 | Community ×3 (channel and seven notes) | TM, BL, GH; WB (36–39 on channel 10) |
| **Pad 2** | **41** declared; the dump reads 32 | **Conflict**: BL, GH say 41; TM says 32 | Open question 1 |
| Which physical pad is the first | not found; the order is the sources' | — | — |
| Keys | channel 1 | Community ×2 | TM, BL |
| Sustain | CC 64 (not declared: a pedal) | Community ×2 | TM, BL |
| Ports | the keys' port `V25` / `V49` / `V61` (Windows), `V25 MIDI 1` (Linux); the editor's `MIDIIN2 (V49)`, `V25 MIDI 2`, `EDITOR` | Community ×4 | PN; TM |
| Identity Reply | **not found** | — | The names ("v25"/"v49"/"v61", not "mkii", not the editor's port) single the family out |
| The pads play when nothing maps them | drum pads on a keyboard | Convention | catalog README |
| Slots | knobs `control-1.1`–`1.4`; pads by note (36–39 `switch-1.5`–`1.8`, 41–42 `switch-1.2`–`1.3`, 46 `switch-2.7`, 49 `switch-2.2`); buttons none (the keyboard has pads); modulation wheel `mod-wheel` | Convention | catalog README |
| Roles | knobs as the first four of the rule for eight knobs and no faders | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Pad 2's note**: 41 or 32.
2. **Which physical pad is the first.**
3. **The Linux port names** on a kernel that names ports after the USB jacks
   (`V25 In`, `EDITOR In`).

## How to check on hardware

Open Controllers in RackForge with the keyboard connected, choose it and
press **Check controls**. Turn every knob, press every button twice and hit
every pad; the report lists what arrived against the rows above. A
difference goes here as a correction, with the firmware version.
