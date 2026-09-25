# Novation Launchkey [MK4] — sources

The packages in `25/`, `37/`, `49/` and `61/` describe the keyboard in
**standalone (MIDI) mode** with its **factory Custom Modes** and its
**power-on modes**. That is the state it starts in, and the one RackForge sees
on its MIDI interface. No value was measured on hardware: each one comes from
Novation's own documentation, listed below.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launchkey MK4 Programmer's Reference Guide, Version 3.0 (27 Oct 2025) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_programmer_s_reference_guide-pdf-en_0.pdf | 2026-09-24 | `35eaefa26784e531095c73a41e0e65f514f532e4bdd73587d14cfd60019a5660` |
| UG25 | Launchkey 25 [MK4] User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_25_user_guide_v3_en.pdf | 2026-09-24 | `dfa3ce9dab768c015a87090cdc9f4a117e35b8a45b46ec1e8393c87def1edbe9` |
| UG37 | Launchkey 37 [MK4] User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_37_user_guide_v3_en.pdf | 2026-09-24 | `a52f1a21a1bd71ef5b242e0cee1345de9d0a9116fdc161b91cac3c4080261e5f` |
| UG49 | Launchkey 49 [MK4] User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_49_user_guide_v3_en.pdf | 2026-09-24 | `304142d233a6f3ce87ccfe39a9356e18116d163d48f3cb78a7d7842e349960c5` |
| UG61 | Launchkey 61 [MK4] User Guide v3 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_61_user_guide_v3_en.pdf | 2026-09-24 | `75aa226a99dae07d1f1be7654b9d2eb1fd431c7cce9a311f8917393d921007c7` |
| CG | Launchkey MK4 Components Guide (Novation support article, with screenshots of the Components editor) | https://support.novationmusic.com/hc/en-gb/articles/20229018847506-Launchkey-MK4-Components-Guide | 2026-09-24 | — (web page) |
| FL | Launchkey MK4 FL Studio Setup (Novation support article, with screenshots of the MIDI port list) | https://support.novationmusic.com/hc/en-gb/articles/20499018257554-Launchkey-MK4-FL-Studio-Setup | 2026-09-24 | — (web page) |
| AL | Launchkey MK4 Mini Ableton Live Setup (Novation support article; the sister Mini, same driver) | https://support.novationmusic.com/hc/en-gb/articles/20213048820626-Launchkey-MK4-Mini-Ableton-Live-Setup | 2026-09-24 | — (web page) |

The PDFs are listed on the official download pages, for example
https://downloads.novationmusic.com/novation/launchkey-mk4/launchkey-mk4-49.
Versions 1.0 and 2.0 of the Programmer's Reference were checked too. None of
the three documents a Device Inquiry reply.

The user guides give the same facts on the same pages for the 49 and the 61,
and for the 25 and the 37. The page numbers below are the 49's, with the 25's
in brackets where they differ.

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's software), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces. The first, MIDI, carries the keys, wheels, pads, and pot and fader Custom Modes; the second, DAW, is for DAWs | exclude `daw`, `midiin2` | Documented | PR p5 |
| Port names: Windows "Launchkey MK4 61 MIDI" and "MIDIIN2 (Launchkey MK4 61 MIDI)"; Mac "Launchkey MK4 49 MIDI Out" and "Launchkey MK4 49 DAW Out" | the size follows "MK4", so `mk4 25`, `mk4 37`, `mk4 49`, `mk4 61` tell the sizes apart | Screenshot | FL |
| On Windows the DAW interface can also appear as "… MIDI (Port 2)" | exclude `port 2` | Screenshot, for the Launchkey Mini MK4 with the same Novation USB driver; not shown for this model | AL |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | PR v1, v2, v3 |
| Powers up in Standalone mode; DAW interface unused there | — | Documented | PR p6 |
| Power-on modes | Encoder: Custom 1; Pad: Drum; Fader: Custom 1 (49/61) | Documented | UG p52 (p45) |
| Encoders, factory Custom Mode | page 1 CC 21–28, page 2 CC 29–36, Control Change, 7 bits, 0–127, Global Channel | Screenshot | CG ("Encoders") |
| Encoders send absolute values in Custom Modes | `encoder = "absolute"` | Screenshot (min 0, max 127); PR p13 reserves the relative output for the DAW Transport mode | CG; PR p12–13 |
| Encoder bank buttons switch page 1 and page 2 of a Custom Mode | — | Documented | UG p22 |
| Faders, factory fader Custom Mode | CC 71–79, Global Channel | Screenshot | CG ("Faders & Buttons") |
| Fader buttons, factory fader Custom Mode | CC 11–19, momentary, on 127 / off 0, Global Channel | Screenshot | CG ("Faders & Buttons") |
| Faders and fader buttons exist only on the 49 and 61 | — | Documented | PR p11; UG49 p9 |
| Keys channel (Part A on 49/61) | 1 | Documented | UG p51 (p45) |
| Global Channel = the keys channel | channel 1 | Convention (Components offers "Global Channel" beside 1–16) | CG |
| Pads in Drum mode | notes 36–51, channel 10 | Documented (channel 10: UG p24, p51; notes: PR p12) | UG p24 (p21); PR p12 |
| Drum layout | top row 40 41 42 43 48 49 50 51, bottom row 36 37 38 39 44 45 46 47 | Documented (DAW Drum mode, which "can replace the Drum mode of standalone (MIDI) mode") | PR p12 |
| The guide's "C1 to D2" for the pads | 36 to 50 is 15 notes for 16 pads; the PR's 36–51 is declared | Discrepancy in Novation's text, noted | UG p24 (p21) |
| Standalone buttons, Control Change on channel 16 | Track left 103, Track right 102, Capture MIDI 74, Undo 77, Quantise 75, Metronome 76, Loop 118, Record 117 | Documented (figure; names from the hardware overview) | PR p6 figure 1; UG p9–10 |
| Play and Stop in standalone | MIDI Real Time Start and Stop: `realtime = "start"` and `"stop"`, taking the transport actions | Documented | PR p7 |
| Value those buttons send on press and release | 127 / 0 assumed | **Not documented** for these buttons; 127/0 is the momentary default Components shows for Custom Mode buttons | CG |
| Pad bank ▲▼, >, Function, Encoder bank ▲▼, Shift, Settings, Scale, Chord Map, Arp, Fixed Chord, Octave −/+ | not declared: absent from the channel 16 figure | Documented (by omission) | PR p6 |
| Modulation wheel | CC 1 by default | Documented | CG ("Mod Wheel/Strip") |
| Pitch wheel | pitch bend on the keys channel | Documented | UG p9 |
| Sustain input | CC 64 by default | Documented | CG ("Pedal") |

## Open questions, until someone with the hardware checks

1. **Track left and right.** The figures give 103 to the left button and 102
   to the right one, in both the standalone and the DAW diagrams (PR p6, p9).
   The Launchkey [MK3] had them the other way round. The package follows the
   MK4 figure.
2. **Whether the factory Custom Modes 1–4 all send the CCs in the screenshots.**
   The Components guide shows a new Custom Mode. A factory reset "resets all
   Custom Modes to their factory state" (CG), without listing that state.
3. **Port names on Linux and Android.** Novation shows Windows and macOS only.
   The matcher asks for "launchkey" and "mk4 <size>" and rejects "mini", "daw",
   "midiin2", "port 2" and "midi 2 " (a second port numbered by the system). If a system
   names the interface without the size, the keyboard is left unclaimed rather
   than given the wrong size.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version the bootloader screen shows
(hold Octave − and Octave + while powering up, PR p4).
