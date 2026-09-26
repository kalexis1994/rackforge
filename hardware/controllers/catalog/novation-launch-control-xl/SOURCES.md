# Novation Launch Control XL (MK1 and MK2) — sources

`rackforge-controller.toml` describes **User Template 1 as it leaves the
factory**, on the controller's MIDI port. It covers the MK1 and the MK2:
Novation documents them together, with one editor and one programmer's
reference. The Launch Control XL 3 (2025) is a different product with its own
documentation, and is not described here.

No value was measured on hardware: each one comes from Novation's own
documentation, listed below.

## Why User Template 1 and not a Factory Template

The Launch Control XL has 8 user templates and 8 factory templates. Novation
publishes the contents of only one of them: the default user template, which
is what the editor creates for a new template ("this new template is the
default User template that comes by default on the unit", CG), with a
screenshot of every value. The factory templates "output a fixed set of MIDI
CCs … and Notes" (GSG p5), but no Novation document lists that set, nor says
which template is active at power-up. The package describes what is published.

RackForge selects User Template 1 itself when the controller connects, with
the "Change current template" message (PR p8): `F0 00 20 29 02 11 77 00 F7`.
By hand, hold **User** and press the first pad of the bottom row (GSG p5).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launch Control XL Programmer's Reference Guide, Version 2 (Dec 2022) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launch_control_xl_programmer_s_reference_guide.pdf | 2026-09-24 | `076985fa9a0859a2ecce0c35d1e843fc5815bd67062c0019de9a8cdca07f7c06` |
| PR1 | Launch Control XL Programmer's Reference Guide, first edition (May 2014) | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/launch-control-xl-programmers-reference-guide.pdf | 2026-09-24 | `98b6183c4d03fcf7b64a8db2f33f50f63ab192af689613a3b633372d814be0fe` |
| GSG | Launch Control XL Getting Started Guide v2 | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/Launch%20Control%20XL%20GSG%20v2.pdf | 2026-09-24 | `5ec473be4cefae0f694171a02daef686c134791d6eb7eb2a2e71d6a36e48cb1f` |
| CG | Launch Control XL MK 1 and 2 Components guide (Novation support article; "Show Values" screenshot of the default template) | https://support.novationmusic.com/hc/en-gb/articles/4411807214226-Launch-Control-XL-MK-1-and-2-Components-guide | 2026-09-24 | — (web page) |
| AL | How to set up Launch Control XL MK1 and MK2 with Ableton Live (port names on Windows and macOS) | https://support.novationmusic.com/hc/en-gb/articles/20807148156690-How-to-set-up-Launch-Control-XL-MK1-and-MK2-with-Ableton-Live | 2026-09-24 | — (web page) |
| AL3 | Launch Control XL 3 Ableton Live setup (the XL 3's port names, to keep it out) | https://support.novationmusic.com/hc/en-gb/articles/27282166560914-Launch-Control-XL-3-Ableton-Live-setup | 2026-09-24 | — (web page) |

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's software), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| One MIDI port, "Launch Control XL n", n the device ID, not shown for ID 1 | `name_contains = ["launch control xl"]` | Documented | PR p3 |
| The second interface: "MIDIIN2 (Launch Control XL)" on Windows, "Launch Control XL (HUI)" on macOS | exclude `midiin2`, `hui` | Documented | AL |
| The Launch Control XL 3's ports are "LCXL3 1 MIDI" and "... (Port 2)" | never contain "launch control xl" | Documented | AL3 |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | PR, PR1 |
| 24 pots, 8 faders, 16 channel buttons, 4 directional buttons, Device, Mute, Solo, Record Arm | — | Documented | PR p3 |
| Templates: user 1–8, factory 1–8; hold User or Factory and press a bottom-row pad to choose | — | Documented | PR p3; GSG p5 |
| The default user template is the one the editor creates for a new template | — | Documented | CG |
| Send A pots | CC 13–20, channel 1, 0–127 | Screenshot | CG ("Show Values") |
| Send B pots | CC 29–36, channel 1 | Screenshot | CG |
| Pan/Device pots | CC 49–56, channel 1 | Screenshot | CG |
| Faders | CC 77–84, channel 1 | Screenshot | CG |
| Track Focus buttons (top row) | notes F1 F♯1 G1 G♯1 A2 A♯2 B2 C3, channel 1, momentary | Screenshot | CG |
| Track Control buttons (bottom row) | notes C♯4 D4 D♯4 E4 F5 F♯5 G5 G♯5, channel 1, momentary | Screenshot | CG |
| Note names to numbers | middle C = C3 = 60, so 41–44 57–60 and 73–76 89–92 | Convention (Novation's naming, stated in its later programmer's references; the editor does not restate it) | Launchkey MK4 PR p3 |
| Send Select ▲▼, Track Select ◀▶ | CC 104, 105, 106, 107, channel 1 | Screenshot (names from the panel print in the PR p8 photo) | CG; PR p8 |
| Device, Mute, Solo, Record Arm | notes A6 A♯6 B6 C7 = 105–108, channel 1 | Screenshot (order from PR p7: Device, Mute, Solo, Record Arm) | CG; PR p7 |
| A button sends 127 on press and 0 on release | 127 / 0 | Documented | PR p8 |
| Change current template | `F0 00 20 29 02 11 77 <template> F7`, template 00h–07h user, 08h–0Fh factory; sent on connect with 00h | Documented | PR p8 |
| Pots are absolute, with a centre detent | knobs | Documented | GSG p3 |
| Held from instruments | every control (`plays = false`): no keys; its buttons send notes, its knobs and faders an instrument's controllers | Convention | catalog README (added 2026-09-25) |

## Open questions, until someone with the hardware checks

1. **The factory templates.** Which messages they send. RackForge leaves them
   alone: it selects User Template 1 on connect. A player who switches to a
   factory template afterwards leaves what this package describes.
2. **Linux and Android port names.** Novation shows Windows and macOS only.

## How to check on hardware

In RackForge, open Controllers with the controller connected and on User
Template 1, and move each control: the MIDI activity must show the messages
above. A difference goes here as a correction, with the firmware version.
