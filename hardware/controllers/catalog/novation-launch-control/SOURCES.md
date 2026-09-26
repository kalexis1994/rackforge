# Novation Launch Control (the first, 2014) — sources

The package describes the Launch Control on **factory template 1**, which
RackForge selects on connect. No value was measured on hardware.

Novation's Programmer's Reference Guide gives:
- the port;
- how templates are selected;
- the channels factory templates speak on;
- how pads and buttons report.

It does not table the factory templates' controls. Those numbers come from
Ableton's Launch_Control script, which selects the same template with the
same message: one official source. Novation's Getting Started Guide describes
the three templates Ableton uses by function only.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| PR | Launch Control Programmer's Reference Guide | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/launch-control-programmers-reference-guide.pdf | 2026-09-25 | `759108827d49557ae41fe4add63d767bfde46378f07b19707b3133cab453f659` |
| GSG | Launch Control Getting Started Guide v2 | https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/Launch%20Control%20GSG%20v2.pdf | 2026-09-25 | `8b1a45ba5d7fd88fa3874e89af87e36b1c21bb582ea7467492c38c1be50b782b` |
| AB | Ableton Live 12 MIDI Remote Script `Launch_Control` (decompiled): `LaunchControl.py`, `Sysex.py`, `__init__.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Launch_Control (commit `0336151d`) | 2026-09-25 | `128f1661…d1f27a`, `89853e75…0ac6`, `a033aee3…e5235bb` |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Controls | 16 knobs, 8 pads, 4 buttons, 2 template buttons | Documented | PR p2; GSG p1 |
| Port | one, `Launch Control`, or `Launch Control n` for device ID n | Documented | PR p2 |
| Other Launch Controls | the XL names itself "Launch Control XL", the XL 3 "LCXL3", the Launch Control 3 "LC3 1 MIDI" | Documented; catalog | catalog packages; Novation's FL Studio setup article for the Launch Control 3 |
| Templates | 8 user (00h–07h, channels 1–8), 8 factory (08h–0Fh, channels 9–16) | Documented | PR p2, p5 |
| Select a template | `F0 00 20 29 02 0A 77 <template> F7`; the device answers with the same message | Documented; Official software | PR p7; AB `Sysex.MIXER_MODE = … 119, 8, 247` |
| Pads and buttons | a note or a control change, 7Fh on press, 0 on release | Documented | PR p7 |
| Factory template 1: knobs | top row CC 21–28, bottom row CC 41–48, channel 9, absolute | Official software (one source) | AB `make_all_encoders`, channel 8 zero-based, absolute |
| Factory template 1: pads | notes 9–12, 25–28, channel 9 | Official software (one source) | AB `pad_identifiers`, `is_pad=True` (notes), channel 8 zero-based |
| Factory template 1: buttons | up 114, down 115, left 116, right 117 | Official software (one source) | AB `Pan_Volume_Mode_Button` 114, `Sends_Mode_Button` 115, `Mixer_Track_Left/Right_Button` 116/117 |
| Held from instruments | every control (`plays = false`): the Launch Control has no keys, and its pads' notes 9–28 are no instrument's | Convention | catalog README |
| Slots | top knobs `control-1`, bottom knobs `control-2` (no faders); pads `switch-1`, in order (their notes are not the Launchkey's); left/right `step`, up/down `step-2` | Convention | catalog README |
| Roles | the eight-knobs-no-faders rule on the top row | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Factory template 1's numbers** rest on Ableton alone.
2. **The Linux port name** (`Launch Control MIDI 1`?): the endpoint matches
   the model's name wherever it appears.
