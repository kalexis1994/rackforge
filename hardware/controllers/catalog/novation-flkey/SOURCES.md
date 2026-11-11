# Novation FLkey (first generation) — sources

The packages in `mini/`, `37/`, `49/` and `61/` describe the first FLkey
generation (2021–2022) used **outside FL Studio**, with its **default Custom
Modes**. The FLkey 2 ("FLkey MK2", 2025) is a different generation and is not
described here. No value was measured on hardware: each one comes from
Novation's own documentation, listed below.

## What Novation publishes, and what it does not

The FLkey is built for FL Studio. Novation publishes user guides and a
Components guide, but **no programmer's reference**. What they establish:

- **Outside FL Studio, the pots have one mode: Custom.** "In standalone
  operation, Channel Rack, Instrument, Sequencer, Plugin, and Mixer:
  Volume/Pan are not available" (UG-Mini p33).
- **The default pot Custom Mode already sends messages** (UG-37 p35; UG-49/61
  p44). The Components guide shows them: CC 21–28.
- **The faders' default Custom Mode** (49 and 61) is shown in a screenshot of a
  new Custom Mode: CC 71–79, with the fader buttons on CC 11–19.

What they do not establish, so the packages leave it out:

- **The pads.** The factory pad Custom Mode is "Notes of the C minor scale"
  (CG), with no note numbers or channel. Which pad mode is active outside FL
  Studio is not stated either.
- **The transport, navigation and workflow buttons.** Every guide describes
  them in FL Studio only.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG-Mini | FLkey Mini User Guide v4 (EN) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/FLkey%20Mini%20User%20Guide%20v4%20English%20-%20EN_1.pdf | 2026-09-24 | `fdf4f37b513f1fe55a0e690c79f2552653ffdde0b9779e7d5a762f64d5ac723b` |
| UG-37 | FLkey 37 User Guide (EN, June 2023) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_37_user_guide-pdf-en.pdf | 2026-09-24 | `ea49a4c3a9a36daad9a30ed2b92862ed29fafc62fbb556e4cfc6d87d8eea7a03` |
| UG-49/61 | FLkey 49 and 61 User Guide (EN, June 2023) | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_49_61_user_guide-pdf-en.pdf | 2026-09-24 | `1ee5b4d25b72817cba352f9eff5382ea7ddcbd2ef8fc3246faaf00fc604b5b27` |
| CG | FLkey Components Guide ("applies to FLkey 61, FLkey 49, FLkey 37 and FLkey Mini", with screenshots of the editor) | https://support.novationmusic.com/hc/en-gb/articles/6741279709714-FLkey-Components-Guide | 2026-09-24 | — (web page) |
| UG2-49 | FLkey 2 49 User Guide v3 (EN), for the FLkey 2's port names only | https://fael-downloads-prod.focusrite.com/customer/prod/downloads/flkey_2_49_user_guide_v3_en.pdf | 2026-09-24 | `27968aae204aca55c69b457216e155e0c7726b6a08e52a3ea546c7137be0f25b` |

The guides are listed on https://downloads.novationmusic.com/novation/flkey.

## Facts, one by one

Evidence levels: **Documented** (stated in text or a table), **Screenshot**
(shown in an official screenshot of Novation's software), **Convention**
(Novation's stated default for the same kind of control, not restated for this
one), **Not documented**.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Two USB MIDI interfaces: "FLkey MIDI Out" and "FLkey DAW Out", the second named MIDIIN2 on Windows | exclude `daw`, `midiin2` | Documented | UG-37 p12; UG-49/61 p12 |
| Full port names carry the model: "FLkey Mini MIDI Out", "FLkey 37 FLkey MIDI Out", "FLkey 61 FLkey MIDI Out" | `flkey mini`, `flkey 37`, `flkey 49`, `flkey 61` | Screenshot (FL Studio's MIDI settings) | UG-Mini p10; UG-37 p11; UG-49/61 p11 |
| The FLkey 2's ports are "FLkey MK2 49 MIDI Out" and "... DAW Out" | exclude `mk2`; never contains "flkey 49" | Screenshot | UG2-49 p13 |
| Device Inquiry reply | — | **Not documented**; no `sysex_identity` | — |
| Outside FL Studio only the Custom pot mode is available | pots declared in their Custom Mode | Documented (Mini) | UG-Mini p33 |
| The pot modes other than Custom act on FL Studio | — | Documented | UG-37 p15; UG-49/61 p15 |
| The default pot Custom Mode sends messages without editing | — | Documented | UG-37 p35; UG-49/61 p44; UG-Mini p33 |
| Pots, default Custom Mode | CC 21–28, 0–127, Global Channel | Screenshot | CG ("Pots") |
| Keys MIDI channel | 1 | Documented (37, 49, 61); Convention (Mini, whose settings do not list it) | UG-37 p36; UG-49/61 p45 |
| Global Channel = the keys channel | channel 1 | Convention | CG |
| Faders and fader buttons exist on the 49 and 61 only | — | Documented | CG ("Faders & Buttons is only used for the FLKey 49, 61"); UG-49/61 p29 |
| Faders, default Custom Mode | CC 71–79, Global Channel | Screenshot (a new Custom Mode) | CG ("Faders") |
| Fader buttons, default Custom Mode | CC 11–19, Global Channel | Screenshot (a new Custom Mode) | CG ("Faders") |
| The other fader modes act on FL Studio | — | Documented | UG-49/61 p29 |
| Values the fader buttons send | 127 / 0 assumed | **Not documented** for the FLkey; the Launchkey MK4's editor shows 127/0 momentary for the same kind of button | — |
| Sustain input | CC 64 by default | Documented | CG ("Sustain Pedal") |
| Pitch wheel (strip on the Mini) | pitch bend | Documented | UG-37 p8; UG-Mini p8 |
| Modulation wheel (strip on the Mini) | CC 1 | Convention (the FLkey guides do not name the message) | UG-37 p8; UG-Mini p8 |
| Pads | not declared | **Not documented** outside FL Studio | CG ("Factory Pad Custom Mode 1: Notes of the C minor scale") |
| Transport, preset, mixer, channel rack, workflow buttons | not declared | **Not documented** outside FL Studio | UG-37 p8–9, p12 |

## Open questions, until someone with the hardware checks

1. **The pot mode at power-up** outside FL Studio. If it is not Custom, hold
   Shift and press the pad labelled Custom (UG-37 p35).
2. **The fader mode outside FL Studio** on the 49 and 61. Their default is
   Mixer Volume, an FL Studio mode (UG-49/61 p29); Custom is selected with
   Shift and its fader button.
3. **What the pads and the buttons send** outside FL Studio.
4. **Windows, Linux and Android port names.** Novation shows FL Studio's list
   on a Mac. A system that names the port without the model leaves the
   keyboard unclaimed.

## How to check on hardware

In RackForge, open Controllers with the keyboard connected and move each
control: the MIDI activity must show the messages above. A difference goes
here as a correction, with the firmware version.
