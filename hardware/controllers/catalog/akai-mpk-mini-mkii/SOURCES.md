# Akai Professional MPK mini mkII — sources

The package describes the MPK mini mkII **on factory Program 1**. No value
was measured on hardware for this package.

**Akai publishes no table of its factory programs.** The strongest source is
a set of factory SysEx dumps of Programs 1–4, read back from a real unit
after a firmware update restored them ([DUMP]); several independent
community mappings written "for the factory defaults" agree with them. The
values are marked Community. The package sends nothing to the keyboard.

Ableton's MPK_mini_mkII script describes factory **Program 2** (knobs CC
20–23, 16–19; pads on channel 10), not Program 1, and is used only for the
model name.

Akai's editor was not opened: its licence forbids reverse engineering.
Akai's *Editor User Guide* (a PDF) shows the editor's blank template, which
matches the knobs, joystick and pad notes below but differs elsewhere; it is
not taken as Program 1.

## Documents

| Tag | Document | Where | Retrieved | sha256 / commit |
|---|---|---|---|---|
| UG | MPK mini User Guide v1.0 (mkII) | https://cdn.inmusicbrands.com/akai/mpk-mini-mk2/MPK_mini_-_User_Guide_-_v1.0.pdf | 2026-09-25 | `99f733a7a009f7b82581363824b5ee4237e3d7b39f60c62f0de759d20eb27355` |
| EG | MPK mini MKII Editor User Guide v1.0 (PDF) | https://cdn.inmusicbrands.com/akai/mpk-mini-mk2/MPK_mini_MKII_Editor_-_User_Guide_-_v1.0.pdf | 2026-09-25 | `eb346afd0deebbd89d9b79706168c17c2dabb40f3e78bb391a1d7d989dd0b2dd` |
| DUMP | mpd-utils: `preset_mk2/preset{1..4}.mk2` (factory dumps, added in `537a53bc`), `preset_mk2/readme.txt`, `sysex/sysex_mk2.md` L2–11 | github.com/mungewell/mpd-utils | 2026-09-25 | commit `4d9dedf3` (Community) |
| MO | Modality toolkit, `Modality/MKtlDescriptions/akai-mpkmini2.desc.scd` L13–22, L41–84 ("Uses default presets") | github.com/ModalityTeam/Modality-toolkit | 2026-09-25 | commit `b783a1ac` (Community) |
| MZ | midizap, `examples/MPKmini2.midizaprc` L17–20, L36–70 ("assumes that the MPKmini2 is set to factory defaults") | github.com/agraef/midizap | 2026-09-25 | commit `b6068a73` (Community) |
| SCR | super-controller, `src/shared/drivers/mpkmini2.ts` L36, L76–109, L158–165 | github.com/aolsenjazz/super-controller | 2026-09-25 | commit `843fb0ae` (Community) |
| GB | GarageBand script, `README.md` L19, L25; `MPKmini2.device/config.lua` L53–56 | github.com/anttikekki/akai-mpk-mini-mk2-garageband-midi-controller-script | 2026-09-25 | commit `3ab67661` (Community) |
| SX | Spectrasonics, Omnisphere 3 Hardware Guide, "Setting up the MPK Mini mk2" | https://support.spectrasonics.net/manual/Omnisphere3HW/3/en/topic/setting-up-the-mpk-mini-mk2 | 2026-09-25 | — (a maker of software; Community here) |
| AB | Ableton Live 12 `MPK_mini_mkII` script (`__init__.py` L19–21, `consts.py` L23–30) | github.com/gluon/AbletonLive12_MIDIRemoteScripts | 2026-09-25 | commit `e83d5192` |

## Facts, one by one

Channels from 1; notes as MIDI numbers.

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Program at power-up | Program 1, "typically" | Community ×1 (weak) | SX |
| Keys, knobs and joystick share one channel setting | — | Documented | EG p11–12 |
| That channel | 1 | Community ×2 | DUMP; MO |
| Knobs K1–K8 | CC 1–8, absolute | Community ×5 | DUMP, MZ, MO, SCR, GB |
| Joystick left/right | pitch bend | Community ×3 | DUMP, MO, SCR |
| Joystick up/down | CC 1 both ways | Community ×3 | DUMP (up = down = CC 1), SCR, GB ("K1 and the joystick Y axis both send CC1") |
| K1 and the joystick send one message | declared once, as the modulation wheel (`knob-1`, slot `mod-wheel`) | follows from the rows above | — |
| Pads, bank A and B, in Program 1 | notes 44–51 and 32–39, **channel 1** | Community ×2 | DUMP; MZ |
| The pads are not declared | on channel 1 a pad and the key of the same note are one message; they play as those notes | Convention | — |
| Pads in CC and Prog Change mode | CC 20–35 / PC 0–15, or the reverse: the dump's two readings disagree | **Conflict** | not declared |
| Other buttons (Octave, Arp, Tap, Full Level, Note Repeat, Bank, CC, Prog Change, Prog Select) | act inside the keyboard; no message documented | Documented | UG p4–5 |
| Sustain pedal's CC | not found | — | — |
| Port name | `MPKmini2` (Windows, Mac); `MPKmini2 MIDI 1` (Linux) | Official software (the model name); Community ×3 | AB `__init__.py` model_name; MZ `JACK_IN`; MO `deviceName`; gist tobert/881dd99b (2017, aconnect listing) |
| Identity Reply, for the record | `F0 7E 00 06 02 47 26 00 19 00 …` (34 bytes) | Community ×1 | DUMP `sysex/sysex_mk2.md`. Not used: the name singles the product out |
| Slots | K1 `mod-wheel`; K2–K8 `control-1.2`–`1.8` in their places (no faders) | Convention | catalog README |
| Roles | K2–K8 as the rule for eight knobs and no faders, in their places; K1 none (it is the modulation wheel) | Convention | catalog README |
| The knobs play when nothing maps them | a keyboard's knobs | Convention | catalog README |

## Open questions, until someone with the hardware checks

1. **Whether the unit always starts on Program 1**, or on the last program
   used.
2. **The pads' CC and Program Change values** (see the conflict above).
3. **The joystick's up/down**: whether both directions send 0→127.
4. **The sustain pedal's** CC.

## How to check on hardware

Select Program 1 (Prog Select + pad 5), open Controllers in RackForge,
choose the MPK mini mkII and press **Check controls**. Turn every knob, move
the joystick both ways and hit the pads; the report lists what arrived
against the rows above. A difference goes here as a correction, with the
firmware version.
