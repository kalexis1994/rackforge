# Novation Launchkey [MK3] — sources

The packages in `25/`, `37/`, `49/`, `61/` and `88/` describe the keyboard in
**standalone (MIDI) mode** with its **factory Custom Modes**. That is the state
it powers up in and returns to after a factory reset, and the one RackForge
sees on its MIDI interface. No value was measured on hardware: each one comes
from Novation's own documentation, listed below.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launchkey [MK3] Programmer's Reference Guide, Version 1 | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk3_programmer_s_reference_guide_v1_en.pdf | 2026-09-24 | `2fffae2203485c29ca52c05f96410fa1476c242969a57b7e023dd87a2a0e9041` |
| UG | Launchkey [MK3] User Guide v6 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/Launchkey%20MK3%20User%20Guide%20v6%20-%20EN.pdf | 2026-09-24 | `ce143afa3c998d8b6cf6b0d5dab660e6cc3de408d12fd34024486be4af5d7e86` |
| CG | Launchkey MK3 Components Guide (Novation support article, with screenshots of the Components editor) | https://support.novationmusic.com/hc/en-gb/articles/360014619639-Launchkey-MK3-Components-Guide | 2026-09-24 | — (web page) |

Both PDFs are listed on the official download page,
https://downloads.focusrite.com/novation/launchkey-mk3/launchkey-49-mk3.

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's editor), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces: MIDI (performance: keys, wheels, pads, pots, faders in Custom Modes) and DAW; on Windows the second is the DAW interface | exclude `daw`, `midiin2` | Documented | PR p4 |
| Device Inquiry reply | `F0 7E 00 06 02 00 20 29 <dev_type> 01 00 00 <app version ×4> F7` | Documented | PR p4 |
| dev_type per model | 25: 34h · 37: 35h · 49: 36h · 61: 37h · 88: 40h | Documented | PR p4 |
| Identity as RackForge reads it | manufacturer `00 20 29`; family = dev_type \| 01h≪7 (25 B4h, 37 B5h, 49 B6h, 61 B7h, 88 C0h); model 0 | Derived from the reply above (family and model are 7-bit pairs, LSB first) | PR p4 |
| Powers up in Standalone mode; DAW interface unused there | — | Documented | PR p5 |
| Standalone buttons send CC on channel 16: Device Select 51, Device Lock 52, Track ◄ 102, Track ► 103, Scene Launch (>) 104, Stop/Solo/Mute 105, ▲ 106, ▼ 107, Capture MIDI 74, Quantise 75, Click 76, Undo 77, Play 115, Stop 116, Record 117, Loop 118 | as listed | Documented (diagram; names from the hardware overview) | PR p5; UG p8–10 |
| Value those buttons send on press and release | 127 / 0 assumed | **Not documented** for these buttons; 0/127 is the Off/On default Components shows for Custom Mode buttons | CG pads screenshot |
| Shift, Settings, …, Fixed Chord, Arp, Scale, Octave −/+ | not declared: absent from the channel 16 table, they act inside the keyboard | Documented (by omission) | PR p5; UG p8 |
| In standalone only Custom Modes exist for pots and faders | — | Documented | UG p34 |
| Pots, factory pot Custom Mode | CC 21–28, global channel | Screenshot | CG ("Pots" screenshot, "Reset to Defaults" panel) |
| Faders, factory fader Custom Mode | CC 71–79, global channel; fader 9 is Master | Screenshot | CG ("Faders and Buttons" screenshot); UG p9 |
| Fader buttons, factory fader Custom Mode | CC 11–19, global channel; button 9 is Arm/Select | Screenshot | CG; UG p9 |
| Faders, fader buttons and Arm/Select exist only on 49, 61, 88 | — | Documented | UG p7 |
| Global channel = keys MIDI channel, default 1 | channel 1 | Documented | UG p41; CG |
| Pads in Drum mode | notes C1–D#2 (36–51) | Documented | UG p28 |
| Drum layout | top row 40 41 42 43 48 49 50 51, bottom row 36 37 38 39 44 45 46 47 | Documented (DAW Drum mode, which "replaces the Drum mode of Standalone mode") | PR p9 |
| Drums MIDI channel | 10 | Documented | UG p41 |
| Pitch and modulation wheels | pitch bend; modulation CC 1, on the keys channel | Convention (standard messages; the guide names the wheels, not their messages) | UG p8 |
| Sustain pedal | CC 64 by default | Documented | CG ("Sustain Pedal") |
| Pots are pots (absolute), not endless encoders | absolute | Documented ("Pot Pickup" setting exists for them) | UG p41 |

## Open questions, until someone with the hardware checks

1. **Which Custom Mode is active at power-up** for the pots and the faders, and
   whether Novation's four factory Custom Modes all send the CCs in the
   screenshots. The Components guide shows the values in the editor with a
   "Reset to Defaults" panel. For the Launchkey Mini MK3, Novation states that
   the factory Custom Mode is the one a new Custom Mode starts from; the MK3
   guide does not repeat that.
2. **Which pad mode is active at power-up.** The pads are declared as they are
   in Drum mode. In Scale Chord, User Chord or a Custom Mode they send other
   messages, and a mapping to them simply does not fire.
3. **The port names on each system.** Novation names the interfaces "MIDI" and
   "DAW" (and, in an earlier edition, "LKMK3 MIDI" and "LKMK3 DAW"), but not
   the full names Windows, Linux or Android show. The matcher therefore asks
   for "launchkey" and "mk3" and rejects "mini", "daw", "midiin2" and "flkey",
   and tells the five sizes apart by the Identity Reply. A size that does not
   answer the Identity Request is left unclaimed rather than guessed.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version the keyboard reports.
