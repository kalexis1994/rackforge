# Novation Launchkey Mini [MK4] — sources

The packages in `25/` and `37/` describe the keyboard in **standalone (MIDI)
mode** with its **factory Custom Modes** and its **power-on modes**. That is
the state it starts in, and the one RackForge sees on its MIDI interface. No
value was measured on hardware: each one comes from Novation's own
documentation, listed below.

Unlike the Launchkey Mini [MK3], the Mini MK4 is covered by Novation's
Programmer's Reference, the same one as the Launchkey [MK4] (see
`../novation-launchkey-mk4/SOURCES.md`).

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launchkey MK4 Programmer's Reference Guide, Version 3.0 (27 Oct 2025); covers the "Mini SKUs" | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_programmer_s_reference_guide-pdf-en_0.pdf | 2026-09-24 | `35eaefa26784e531095c73a41e0e65f514f532e4bdd73587d14cfd60019a5660` |
| UG25 | Launchkey Mini 25 MK4 User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_mini_25_user_guide_v3_en.pdf | 2026-09-24 | `45539b21163064fe420d1faf2e058368e37820de052ee096af5e516a504f220e` |
| UG37 | Launchkey Mini 37 MK4 User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_mini_37_user_guide_v3_en.pdf | 2026-09-24 | `40ad92cb9d5911de26898b29dba4bcbbea04912af2782de6c3c1111c137e89fa` |
| CG | Launchkey MK4 Components Guide; "applies to Launchkey MK4, Launchkey MK4 Mini" | https://support.novationmusic.com/hc/en-gb/articles/20229018847506-Launchkey-MK4-Components-Guide | 2026-09-24 | — (web page) |
| AL | Launchkey MK4 Mini Ableton Live Setup (with screenshots of the MIDI port lists) | https://support.novationmusic.com/hc/en-gb/articles/20213048820626-Launchkey-MK4-Mini-Ableton-Live-Setup | 2026-09-24 | — (web page) |

The two user guides give the same facts on the same pages.

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's software), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces: MIDI (performance) and DAW | exclude `daw`, `midiin2` | Documented | PR p5 |
| Port names on Windows: "Launchkey Mini MK4 25 MIDI", "MIDIIN2 (Launchkey Mini MK4 25 MIDI)"; in Live 12 the second shows as "Launchkey Mini MK4 25 MIDI (Port 2)" | exclude `port 2` | Screenshot | AL (Live 10 and Live 12, Windows) |
| Port names on macOS: "Launchkey Mini MK4 37 (MIDI Out)", "Launchkey Mini MK4 37 (DAW Out)" | — | Screenshot | AL (Live 10, macOS) |
| The size follows "Mini MK4" | `mini mk4 25`, `mini mk4 37` | Screenshot (both sizes shown) | AL |
| The article also says the DAW ports may appear as "MIDIIN2 (LKMK4 MIDI)" | a MIDI port named "LKMK4 MIDI" has neither "Launchkey" nor the size, and is left unclaimed | Documented (text only) | AL |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | PR v1, v2, v3 |
| Powers up in Standalone mode | — | Documented | PR p6 |
| Power-on modes | Encoder: Custom 1; Pad: Drum | Documented | UG p48 |
| Encoders, factory Custom Mode | page 1 CC 21–28, page 2 CC 29–36, 7 bits, 0–127, Global Channel | Screenshot, taken with a Launchkey MK4 in the editor; the article covers both products and the Mini has the same eight encoders with two pages | CG ("Encoders"); UG p19 |
| Encoder bank buttons switch page 1 and page 2 | — | Documented | UG p19 |
| Keys channel | 1 | Documented | UG p48 |
| Global Channel = the keys channel | channel 1 | Convention | CG |
| Pads in Drum mode | notes 36–51, channel 10; top row 40 41 42 43 48 49 50 51, bottom row 36 37 38 39 44 45 46 47 | Documented (channel: UG p21, p48; notes and layout: PR p12) | UG p21; PR p12 |
| The guide's "C1 to D2" for the pads | 15 notes for 16 pads; the PR's 36–51 is declared | Discrepancy in Novation's text, noted | UG p21 |
| The one standalone button on the Mini: Record | Control Change 117, channel 16 | Documented (the Mini diagram) | PR p6 figure 1; UG p9 |
| Play | MIDI Real Time Start; Shift + Play sends Stop: `realtime = "start"` and `"stop"`, taking the transport actions | Documented | PR p7 |
| Value Record sends on press and release | 127 / 0 assumed | **Not documented**; 127/0 is the momentary default Components shows | CG |
| Pad bank ▲▼, >, Func., Encoder bank ▲▼, Arp, Scale, Shift, Settings, Octave −/+ | not declared: absent from the Mini diagram | Documented (by omission) | PR p6 |
| Pitch touch strip | pitch bend on the keys channel | Documented | UG p9 |
| Modulation touch strip | CC 1 by default | Documented | CG ("Mod Wheel/Strip", with a Mini screenshot) |
| Sustain input | CC 64 by default | Documented | CG ("Pedal"); UG p5 |

## Open questions, until someone with the hardware checks

1. **Whether the factory encoder Custom Modes on the Mini send CC 21–36**, as
   the Components screenshot taken with a Launchkey MK4 shows.
2. **Port names on Linux and Android.** The matcher asks for "launchkey" and
   "mini mk4 <size>" and rejects "daw", "midiin2", "port 2" and "midi 2 ". A
   system that shows only "LKMK4 MIDI" leaves the keyboard unclaimed.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version.
