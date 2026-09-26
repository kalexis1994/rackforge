# Akai Professional LPD8 and LPD8 mk2 — sources

Two packages: `lpd8/` describes the first LPD8 (2009) on **Program 1**,
`lpd8-mk2/` the LPD8 mk2 on **Program 1**, the program it starts in. No
value was measured on hardware for these packages. The LPD8 Wireless is not
described.

**Akai publishes no factory values for either.** Its guides give the
layout and send the player to the editor. The first LPD8's Program 1 is read
by Ableton's script and by several community sources; the mk2's values are
community only. Two sources learnt the editor's protocol by watching its USB
traffic and read the settings back from a unit (marked *sniff*); nobody
extracted Akai's editor, whose licence forbids reverse engineering. The
packages send nothing to the device.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| QS | LPD8 Quickstart Guide RevA (English PDF in the zip) | https://cdn.inmusicbrands.com/akai/attachments/lpd8/lpd8___quickstart_guide___reva_00.zip | 2026-09-25 | zip `5fcf128285e8631c90bba78c85c0130c93208963f79b61930cf2f5929b375538`; PDF `5588e0c4f905b4dd64a2a086e6ca8dfe81a59845c1e2ea5408ac926cf77af042` |
| EG | LPD8 Editor User Guide v1.0 (PDF) | https://cdn.inmusicbrands.com/akai/LPD8/LPD8-EditorUserGuide-v1.0.pdf_062c35bed20bc085907692f4539a918c.pdf | 2026-09-25 | `2c0c965355bc804500821903fc17a9042d6ea92bd62e7d5dca03a0919ba737c2` |
| UG2 | LPD8 mk2 User Guide v1.2 | https://cdn.inmusicbrands.com/akai/LPD8/LPD8%20mk2%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-25 | `dc4eee2c9294b75886e5b3eb47a3b1d703dcd1a978848fce16e87faf42424b81` |
| KB | Akai knowledge base, articles 69000867977 (the mk2 starts in Program 1, NOTE mode) and 69000867341 (mk2 layout) | support.akaipro.com | 2026-09-25 | — |
| AB | Ableton Live 12 `LPD8` script: `__init__.py` L19–21, `config.py` L41–47, `consts.py` L23–30 | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `e83d5192f321b24eb9daab843ac49a2d95d862b1` |
| MX | Mixxx, `res/controllers/Akai-LPD8-RK.midi.xml` (pads only: its knobs are custom) | github.com/mixxxdj/mixxx | 2026-09-25 | commit `bcfb7956` (Community) |
| JR | AKAI_SC, `LPD8/LPD8.sc` L14–123 | github.com/jreus/AKAI_SC | 2026-09-25 | commit `2031e005` (Community) |
| HP | hue-pad, `hue_pad.py` L89–104, L205–213 | github.com/michael-lazar/hue-pad | 2026-09-25 | commit `96a93618` (Community) |
| CF | lpd8editor, `doc/SYSEX.md` L148–180 (*sniff*) | github.com/charlesfleche/lpd8editor | 2026-09-25 | commit `d0afc23a` (Community) |
| MU | mpd-utils, `sysex/sysex_lpd8.md` | github.com/mungewell/mpd-utils | 2026-09-25 | commit `4d9dedf3` (Community) |
| SM | lpd8mk2, README L7–9, L94–175; `docs/sysex_captures.md` (*sniff*) | github.com/stephensrmmartin/lpd8mk2 | 2026-09-25 | commit `6d62abb0` (Community) |
| WB | mixxx-lpd8-mk2-daedalus, `HARDWARE.md` L42–56, L92–105 (checked on hardware) | github.com/wobondar/mixxx-lpd8-mk2-daedalus | 2026-09-25 | commit `2d857cca` (Community) |
| NL | WebDmxController, `src/lib/inputs/midi/AkaiLPD8MK2Profile.js` L11–36 | github.com/NielsLeenheer/WebDmxController | 2026-09-25 | commit `3d8ebe74` (Community) |
| PB | padbound, `src/padbound/plugins/akai_lpd8_mk2.py` L214–230 (conflicting knob values) | github.com/uermel/padbound | 2026-09-25 | commit `835a0334` (Community) |

## Facts, one by one: LPD8

Channels from 1.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Layout | pads 1–4 the bottom row, 5–8 the top, left to right; knobs K1–K4 the top row, K5–K8 the bottom | Documented | QS; EG |
| Knobs K1–K8, Program 1 | CC 1–8, channel 1, 0–127 | Official software; Community ×4 | AB `consts.py` L23–30; JR, HP, CF, MU |
| Pads, Program 1 | notes 36–43, channel 1 | Official software; Community ×4 | AB `config.py` `PAD_TRANSLATION`, `CHANNEL: 0`; MX, JR, HP, CF |
| Programs 2–4 | channels 2–4 with other notes, in three sources; one unit dumped four identical programs | **Conflict** | JR, HP, MX vs MU; not described: the package is Program 1 |
| Pads in CC mode | CC 1–6, 8, 9, or CC 9–16 | **Conflict** | not declared |
| Aftertouch | none | Documented | QS MIDI chart |
| Program at power-up | **not found** | — | Open question 1 |
| Port | `LPD8`; Linux `LPD8 MIDI 1` | Official software (the name); Community ×3 | AB `model_name`, `INPUTPORT`; CF; elamperti/dotfiles `c468b1d0`; stylemistake/bitwig-lpd8 `4a12f676` |
| Identity Reply | `F0 7E 00 06 02 47 75 00 19 00 …`; Akai's MIDI chart says Device Inquiry "N" | **Conflict** | CF, MU vs QS. Not used: the name singles the product out |

## Facts, one by one: LPD8 mk2

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Layout | as the LPD8 | Documented | UG2; KB 69000867341 |
| Starts in Program 1, NOTE mode | — | Documented (Akai's knowledge base); Community ×1 | KB 69000867977; WB |
| Knobs K1–K8 | CC 70–77, the global channel (1 by default), absolute | Community ×3 | SM, WB, NL. **PB says CC 1–8 on channel 10**: open question 2 |
| Pads, NOTE mode | notes 36–43, channel 10 | Community ×2 | SM, WB |
| Pads in CC and PC mode | CC 12–19, PC 0–7 | Community ×2 | SM, WB; not declared |
| Programs 1–4 | send the same messages; they differ in aftertouch, full level and toggling | Community ×2 | SM, WB |
| Mode and Program buttons | send no MIDI | Community ×1 | WB |
| Port | `LPD8 mk2`; Linux `LPD8 mk2 MIDI 1` | Community | SM, WB, NL |
| Identity Reply | **not found** | — | — |

## For both

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Held from instruments | the knobs (`plays = false`): no keys; CC 1–8 are modulation, breath, volume… to an instrument | Convention | catalog README |
| The pads play | drum pads, as the MPD218's | Convention | catalog README |
| Slots | knobs `control-1` (no faders); pads by note: 40–43 `switch-1.1`–`1.4`, 36–39 `switch-1.5`–`1.8` | Convention | catalog README |
| Roles | the knobs as the rule for eight knobs and no faders | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **The LPD8's program at power-up**, and whether its factory Programs 2–4
   differ from Program 1.
2. **The LPD8 mk2's knobs**: CC 70–77 on channel 1, or CC 1–8 on channel
   10.
3. **Identity Replies** of both.

## How to check on hardware

Select Program 1, open Controllers in RackForge, choose the LPD8 and press
**Check controls**. Turn every knob and hit every pad in PAD (note) mode;
the report lists what arrived against the rows above. A difference goes
here as a correction, with the firmware version.
