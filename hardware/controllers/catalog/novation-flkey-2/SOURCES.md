# Novation FLkey 2 — sources

The packages in `mini-25/`, `37/`, `49/` and `61/` describe the FLkey 2 (2025)
used **outside FL Studio**, with its **power-on modes** and **default Custom
Modes**. The first FLkey generation is in `../novation-flkey/`. No value was
measured on hardware: each one comes from Novation's own documentation, listed
below.

The FLkey 2 documents more than the first generation did. Its settings table
names the modes it starts in (Encoder: Custom 1, Pad: Drum, and Fader: Custom 1
on the 49 and 61), and the Components guide shows the default Custom Modes. It
still has no programmer's reference, so the pads' Drum notes and the buttons
outside FL Studio are unknown and not declared.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG-Mini | FLkey 2 Mini 25 User Guide v9 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_2_mini_25_user_guide_v9_en.pdf | 2026-09-24 | `ae97968a59eaf8336105c89860db49b76dbcb3bc5029a4943941c63d57fc83ec` |
| UG-37 | FLkey 2 37 User Guide v5 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_2_37_user_guide_v5_en.pdf | 2026-09-24 | `b1e3bff96e50af185d3cba4cc44d4d651702897b518bc164ea8cdc87f59a2a16` |
| UG-49 | FLkey 2 49 User Guide v5 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_2_49_user_guide_v5_en.pdf | 2026-09-24 | `06b2934a53e18187b8e1f896cedbea65d294e88a6a50b9bcd5d91f8cbb557bdb` |
| UG-61 | FLkey 2 61 User Guide v5 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_2_61_user_guide_v5_en.pdf | 2026-09-24 | `320564833388c5ae145328059ef4db36a1f54c4c801d9ab3b060c8b8331bd98e` |
| CG | FLkey 2 Components Guide ("applies to FLkey 2, FLkey 2 Mini", with screenshots of the editor) | https://support.novationmusic.com/hc/en-gb/articles/35227065484562-FLkey-2-Components-Guide | 2026-09-24 | — (web page) |

The guides are listed on https://downloads.novationmusic.com/novation/flkey-2.
The 49 and 61 guides give the same facts on the same pages.

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's software), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces: "FLkey MIDI Out" and "FLkey DAW Out", the second named MIDIIN2 on Windows | exclude `daw`, `midiin2` | Documented | UG p13 (all four) |
| Full port names: "FLkey Mini MK2 25 MIDI Out", "FLkey MK2 37 MIDI Out", "FLkey MK2 49 MIDI Out", "FLkey MK2 61 MIDI Out" | `flkey mini mk2 25`, `flkey mk2 37`, `flkey mk2 49`, `flkey mk2 61` | Screenshot (FL Studio's MIDI settings) | UG p13 (all four) |
| The first generation's ports ("FLkey Mini MIDI Out", "FLkey 61 FLkey MIDI Out") contain no "MK2" | never match | Screenshot | see `../novation-flkey/SOURCES.md` |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | — |
| Power-on modes | Encoder: Custom 1; Pad: Drum; Fader: Custom 1 (49/61) | Documented | UG-Mini p65; UG-37 p64; UG-49/61 p73 |
| The default Custom Modes send messages without editing | — | Documented | UG-37 p58; UG-49/61 p66–67; UG-Mini p59 |
| Encoders, default Custom Mode | page 1 CC 21–28, page 2 CC 29–36, Control Change, 0–127, Global channel | Screenshot | CG ("Encoders") |
| Encoders send absolute values in Custom Modes | `encoder = "absolute"` | Screenshot (min 0, max 127) | CG |
| Faders and the fader-and-button Custom Mode exist on the 49 and 61 only | — | Documented | CG ("Faders and Buttons") |
| Faders, default Custom Mode | CC 71–79, Global channel | Screenshot | CG ("Faders and Buttons") |
| Fader buttons, default Custom Mode | CC 11–19, Global channel | Screenshot | CG |
| Keys channel (Part A on the 49 and 61) | 1 | Documented | UG-Mini p64; UG-37 p63; UG-49/61 p72 |
| Global channel = the keys channel | channel 1 | Convention | CG |
| Modulation wheel or strip | CC 1 by default | Documented | CG ("Mod Wheel/Strip") |
| Pitch wheel or strip | pitch bend | Documented | UG p10 |
| Sustain input | CC 64 by default | Documented | CG ("Pedal") |
| Pads in Drum mode, the power-on pad mode | notes not listed ("a chromatic keyboard across the pads", channel 10) | **Not documented** as numbers; not declared | UG-49/61 p30; UG-37 p25; UG-Mini p21 |
| Pads in their default Custom Modes | top row CC 44–51, bottom row CC 36–43, 127/0 momentary | Screenshot; not declared, since the pads start in Drum mode | CG ("Pads") |
| Values the fader buttons send | 127 / 0 assumed | **Not documented** for the fader buttons; the pads' default shows 127/0 momentary | CG |
| Transport, workflow, mixer, channel rack buttons | not declared | **Not documented** outside FL Studio | UG p10–11 |

## Open questions, until someone with the hardware checks

1. **The pads' notes in Drum mode.** The Launchkey MK4, whose settings table
   the FLkey 2 shares, documents 36–51 on channel 10. That is not stated for
   the FLkey 2.
2. **What the buttons send outside FL Studio.**
3. **Windows, Linux and Android port names.** Novation shows FL Studio's list
   on a Mac only.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version.
