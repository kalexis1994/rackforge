# M-Audio Keystation 49 / 61 / 88 MK3 — sources

The packages describe the keyboards **on their MIDI port, as they leave the
factory**. No value was measured on hardware.

A Keystation has few controls. On its MIDI port they are the volume slider,
the two wheels and the sustain pedal (the 88 adds an expression pedal). The
transport and the arrows send on the second port, as Mackie Control, HUI or
MIDI (UG p13). What they send in MIDI mode is not documented, and a user
reports the factory mode is Mackie Control. They are not described.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG49 | Keystation 49 MK3 User Guide v1.6 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Keystation%2049%20MK3%20-%20User%20Guide%20-%20v1.6.pdf | 2026-09-24 | `a7109fc37cd5c3234ba0344f7be3e1e86b1131d615f8ee42771a3dcbc43dbb6f` |
| UG61 | Keystation 61 MKIII User Guide v1.6 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Keystation%2061%20MKIII%20-%20User%20Guide%20-%20v1.6.pdf | 2026-09-24 | `75b5ed769676279a15f11a1152a2aca85a6f0efb592190f9ac9dfcee8ee606dc` |
| UG88 | Keystation 88 MKIII User Guide v1.9 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Keystation%2088%20MKIII%20-%20User%20Guide%20-%20v1.9.pdf | 2026-09-24 | `6f337e0ce2525e78e2fde1072fb59a66aba54c6482fcee068fe56ad0c18d51fe` |
| M32 | Keystation Mini 32 MK3 User Guide v1.0 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/KeystationMini32MK3-UserGuide-v1.0.pdf | 2026-09-24 | `3037208a528320fb5938f81ffd5330a5322cc772662bbda3bdfb504b3d8daa4c` |
| SF | Steinberg forum, "Mapping an M-Audio Keystation MK3" | https://forums.steinberg.net/t/mapping-an-m-audio-keystation-mk3/592587 | 2026-09-24 | — (web page) |
| MT | Modartt forum, a Keystation 88 MK3 owner on its DAW mode LEDs | https://forum.modartt.com/viewtopic.php?id=8767 | 2026-09-24 | — (web page) |

Page numbers below are the 49's (UG49). The 61's and 88's guides have the
same sections a page or so apart.

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| Port names | `Keystation 49 MK3` (the MIDI port); Ableton lists `Keystation 49 MK3 (Port 2)` as the Mackie Control input; Windows 7/8 show "USB Audio Device" | Documented | UG49 p5, p6 |
| Second port on Windows | `MIDIIN2 (Keystation 49 MK3)` | Community | KVR Bitwig thread, as reported by the research; excluded either way |
| Volume slider | sends MIDI volume: CC 7 | The function is Documented ("controls the volume of the notes you are playing", UG49 p6; assignable, p12). The number is **Community**: "The volume slider is by default mapped to MIDI CC 7" (SF). The sister model's guide Documents the same default: "The Volume Knob is assigned the default … (MIDI CC) of 7" (M32) | UG49 p6, p12; SF; M32 |
| Channel | 1 by default: + and − together "will reset to Channel 1" | Documented | UG49 p11 |
| Modulation wheel | CC 1 | Convention: the guide lists CC 01 (Modulation) first among the wheel's assignments, but not as its default | UG49 p13 |
| Pitch bend wheel | pitch bend | Documented | UG49 p6 |
| Sustain pedal | CC 64 | Convention (the guide says it sustains; the number is MIDI's) | UG49 p7 |
| Expression pedal (88 only) | CC 1 at every power-up, the modulation wheel's message: **not declared**, two controls may not send the same message | Documented | UG88 p13 |
| Transport and arrows | second port; MIDI, Mackie Control or HUI, chosen with the DAW key | Documented | UG49 p13 |
| The factory DAW mode is Mackie Control, which a factory reset does not change | "have always been green … I tried a factory reset and they are still yellow" | Community | MT |
| Identity Reply | not documented | — | the size and "MK3" in the port name tell the models apart |
| Role | the volume slider keeps RackForge's master level | Convention (the catalog's master fader) | — |
| Slot | the modulation wheel fills `mod-wheel` | Convention (catalog README) | — |

## Open questions, until someone with the hardware checks

1. **The volume slider's CC** after a factory reset, and its channel when the
   keyboard's channel changes.
2. **What the transport and arrows send in MIDI mode**, to describe them on
   the second port.
3. **The port names on a Mac and on Linux.**
