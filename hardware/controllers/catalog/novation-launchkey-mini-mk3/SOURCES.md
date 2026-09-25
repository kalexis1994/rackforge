# Novation Launchkey Mini [MK3] — sources

`rackforge-controller.toml` describes the keyboard on its **MIDI interface**,
with its **factory Custom Mode** for the knobs and its pads in **Drum mode**.
No value was measured on hardware.

Novation publishes a User Guide and a Components guide for the Mini, but **no
Programmer's Reference**: the Launchkey [MK3] Programmer's Reference covers
the 25, 37, 49, 61 and 88 only. Two consequences:

- **Matching:** the Mini's Identity Reply is not documented, so the package
  matches on the port name alone and asks for "mk3" in it.
- **Pads:** their notes in Drum mode are not stated for the Mini. They are
  taken from the Launchkey [MK3] and marked below as an assumption.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Launchkey Mini [MK3] User Guide 1.1 (EN) | https://fael-downloads-prod.focusrite.com/customer/test/s3fs-public/downloads/Launchkey%20Mini%20MK3%20User%20Guide%201.1%20English%20EN.pdf | 2026-09-24 | `771aeca3146da7d258b3737cd94ded2fde6319b3c061d8d037104e2979766aac` |
| CG | Launchkey Mini MK3 Components Guide (Novation support article, with screenshots of the Components editor) | https://support.novationmusic.com/hc/en-gb/articles/360009345719-Launchkey-Mini-MK3-Components-Guide | 2026-09-24 | — (web page) |
| LK-UG | Launchkey [MK3] User Guide v6 (EN), the larger sister | see `../novation-launchkey-mk3/SOURCES.md` | 2026-09-24 | `ce143afa3c998d8b6cf6b0d5dab660e6cc3de408d12fd34024486be4af5d7e86` |
| LK-PR | Launchkey [MK3] Programmer's Reference Guide, Version 1 | see `../novation-launchkey-mk3/SOURCES.md` | 2026-09-24 | `2fffae2203485c29ca52c05f96410fa1476c242969a57b7e023dd87a2a0e9041` |

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's editor), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**, and **Assumption** (declared anyway, and why).

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces. The second is the DAW port: "Launchkey Mini MK3 (DAW Port)" on a Mac, "Launchkey Mini MIDI IN2" on Windows | exclude `daw`, `midiin2`, `midi in2`, `midi 2 ` (with its space, so "MIDI 20:0" is kept) | Documented (names of the DAW port only) | UG p11 |
| Keys, pads in Custom Mode, knobs in Custom Mode, pitch and modulation come out of the MIDI interface | — | Documented for the keys, knobs and pads | UG p29 |
| Identity Reply | — | **Not documented**; no `sysex_identity` | — |
| The factory Custom Mode is the one a new Custom Mode starts from | — | Documented | CG ("Custom Modes") |
| Knobs, factory knob Custom Mode | CC 21–28, global channel | Screenshot | CG ("New Custom Mode", Pots tab) |
| Knobs send CC only in Knob Custom Mode (Shift + top-right pad); the other knob modes are for a DAW | — | Documented | UG p10, p29 |
| Global channel = keys MIDI channel (Shift + Transpose sets it), default 1 | channel 1 | Screenshot (Components shows "Global Channel"); the default 1 is Convention | UG p8; CG |
| Pads, factory pad Custom Mode | top C3 D3 D♯3 F3 G3 G♯3 A♯3 C4, bottom C2 D2 D♯2 F2 G2 G♯2 A♯2 C3 | Screenshot | CG (Pads tab of a new Custom Mode) |
| Why the pads are not declared in Custom Mode | pad 1 and pad 16 both send C3: a map could not tell them apart | Derived from the row above | CG |
| Pads in Drum mode | notes 36–51, channel 10; top row 40 41 42 43 48 49 50 51, bottom row 36 37 38 39 44 45 46 47 | **Assumption**: the Launchkey [MK3]'s documented Drum mode. The Mini's guide names Drum mode but gives neither notes nor channel | LK-UG p28, p41; LK-PR p9; UG p8, p18 |
| Pitch and modulation touch strips | pitch bend; modulation CC 1, on the keys channel | Convention (standard messages; the guide names the strips, not their messages) | UG p8 |
| Sustain input | CC 64 by default | Screenshot | CG (pedal setting) |
| Play, Record, >, Stop/Solo/Mute, Arp, Fixed Chord, Shift, Transpose, Octave −/+ | not declared | **Not documented** as MIDI on the MIDI interface; the guide gives Play and Record to the DAW and the rest act inside the keyboard | UG p8, p13 |

## Open questions, until someone with the hardware checks

1. **The pads' notes and channel in Drum mode**, and whether the Mini starts in
   Drum mode with no DAW. The Launchkey [MK3] documents 36–51 on channel 10.
2. **Which knob mode is active at power-up** with no DAW. If it is not Custom,
   the knobs are silent on the MIDI interface until Shift + the top-right pad
   selects it.
3. **The MIDI interface's full name on each system.** Novation names only the
   DAW port. The matcher asks for "launchkey", "mini" and "mk3". If a system
   names the interface without "MK3", the keyboard is left unclaimed rather
   than confused with a Launchkey Mini MK1, MK2 or MK4.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version.
