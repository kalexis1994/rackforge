# Novation Launch Control XL 3 — sources

`rackforge-controller.toml` describes **Mode 16**, on the controller's MIDI
interface. No value was measured on hardware: each one comes from Novation's
own documentation, listed below.

## Why Mode 16

Outside a DAW, the Launch Control XL 3 sends MIDI only from a Custom Mode (UG
p25). It has fifteen editable Custom Modes and one that cannot be edited,
Mode 16, "a default set of the following CCs that send on MIDI channel 16" (UG
p88). Novation also points to Mode 16 for mapping the controller by hand (AL).
It is the one layout that is published and cannot drift, so the package
describes it.

RackForge selects Mode 16 itself when the controller connects, through its
DAW port, where the XL 3 takes its feature controls in standalone mode (PR
p16): `9F 0B 7F` turns them on, and `B6 1E 1D` -- surface mode select,
Control Change 1Eh on channel 7 -- chooses Custom Mode 16 (PR p17: Custom
Modes 5–16 are 12h–1Dh). By hand, press **Mode**, the 16th button (bottom
row, far right), and **Mode** again (UG p14).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Launch Control XL 3 User Guide (EN, Jan 2026) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launch_control_xl_3-pdf-en.pdf | 2026-09-24 | `44336116819ce3379b53474b7a342b7bf0e29c6a91036b3b49fe43470f5c361d` |
| PR | Launch Control XL 3 Programmer's Reference Guide, Version 1.0 | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launch_control_xl_3_programmer_s_reference_guide-pdf_en.pdf | 2026-09-24 | `4ac520cae51280b6c21fee35cb6b30ef4f50d80d5e1161d921ffe3c36c2736ad` |
| AL | Launch Control XL 3 and Launch Control 3 – Ableton Live setup (Novation support article, with port lists for Windows and macOS and a Mode 16 diagram) | https://support.novationmusic.com/hc/en-gb/articles/27282166560914-Launch-Control-XL-3-Ableton-Live-setup | 2026-09-24 | — (web page) |

Both PDFs are listed on https://downloads.focusrite.com/novation/launch-control-xl-3/launch-control-xl-3.

## Facts, one by one

Evidence levels: **Documented** (stated in text, a table or a labelled
diagram), **Screenshot** (shown in an official screenshot of Novation's
software), **Convention** (Novation's stated default for the same kind of
control, not restated for this one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| USB interfaces: MIDI (Custom Modes), DAW, and two "To DIN Out" outputs | match the MIDI interface only | Documented | PR p5 |
| Port names on Windows: "LCXL3 1 MIDI", and the DAW port as "LCXL3 1 MIDI (Port 2)" or "MIDIIN2 (...)" | `name_contains = ["lcxl3"]`; exclude `port 2`, `midiin2` | Documented | AL (Windows, Live 12 and Live 11) |
| Port names on macOS: "LCXL3 1 (MIDI Out)" and "LCXL3 1 (DAW Out)" | exclude `daw` | Screenshot | AL (macOS, Live 11) |
| The number in the name is the device ID, 1–8 | any ID is claimed | Documented | PR p4; UG p85 |
| The Launch Control 3 is "LC3 1 MIDI" | never contains "lcxl3" | Documented | AL |
| The Launch Control XL (MK1/MK2) is "Launch Control XL" | never contains "lcxl3" | Documented | see `../novation-launch-control-xl/SOURCES.md` |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | PR |
| Powers up in Standalone mode | — | Documented | PR p7 |
| Which mode is active at power-up | — | **Not documented**; RackForge selects Mode 16 on connect | — |
| Feature controls in standalone mode | on with `9F 0B 7F`, off with `9F 0B 00`, sent to the DAW In port | Documented | PR p16 |
| Surface mode select | Control Change 1Eh, channel 7; Custom Modes 1–4 = 06h–09h, 5–16 = 12h–1Dh, so Mode 16 = 1Dh | Documented (the table on p10 numbers the ranges inconsistently; p17's table is used) | PR p10, p17 |
| Mode 16 cannot be edited and sends on channel 16 | channel 16 | Documented | UG p22, p88 |
| Encoders, Mode 16 | row 1 CC 13–20, row 2 CC 21–28, row 3 CC 29–36 | Documented (labelled diagram) | UG p88; AL |
| Faders, Mode 16 | CC 5–12 | Documented (labelled diagram) | UG p88; AL |
| Buttons, Mode 16 | top row CC 37–44, bottom row CC 45–52 | Documented (labelled diagram) | UG p88; AL |
| Encoders are endless and report absolute values | `encoder = "absolute"` | Documented for DAW mode ("by default encoders are in absolute mode"); Convention for Mode 16 | UG p16; PR p10 |
| Values the buttons send on press and release | 127 / 0 assumed | **Not documented** for Mode 16 | — |
| Page, Track, Record, Play, Solo/Arm and Mute/Select send nothing in standalone mode | not declared | Documented | PR p7 |
| Shift and Mode open menus | not declared | Documented | PR p7; UG p12–14 |
| Held from instruments | every control (`plays = false`): no keys, and the buttons' CC 37–52 are LSBs of an instrument's volume, pan and expression | Convention | catalog README (added 2026-09-25) |

## Open questions, until someone with the hardware checks

1. **Whether Mode 16 holds** once selected through the feature controls, and
   what Custom Modes 1–15 hold from the factory. RackForge leaves the
   feature controls on after selecting it, rather than assume turning them
   off keeps the mode.
2. **The buttons in Mode 16**: momentary or toggle, and their values.
3. **Linux and Android port names.** Novation shows Windows and macOS only.

## How to check on hardware

In RackForge, open Controllers with the controller connected and on Mode 16,
and move each control: the MIDI activity must show the messages above. A
difference goes here as a correction, with the firmware version the
bootloader screen shows (hold both Page buttons while connecting, UG p85).
