# M-Audio Oxygen 25 / 49 / 61 [MKV] — sources

The packages describe the keyboards in the **DAW mode RackForge puts them in
when they connect**, as for the Oxygen Pro. No value was measured on hardware.

**One official source only.** M-Audio's guide names the controls and the DAW
modes but no CC. The only encoding of them is Ableton Live's `Oxygen_5th_Gen`
script, which is the Oxygen Pro script with a mode byte of its own. Every
value from it is marked **Official software (one source)**. What that script
leaves unclear is left out:

- **the pads:** Live retranslates them for its drum rack, so the notes they
  send are not established;
- **the Oxygen 25's single fader:** the script names no fader for the 25;
- **the fader buttons' slots:** whether they report a release is not
  established.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| UG | Oxygen Series MKV User Guide v1.2 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Oxygen%20Series%20MKV%20-%20User%20Guide%20-%20v1.2.pdf | 2026-09-24 | `5ef6ead23cd9a999afb1e7c39995076c26432bfc975910d70b2935bd52b20a40` |
| QS | Oxygen 49 (MKV) Quickstart Guide v1.1 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Oxygen%2049%20-%20Quickstart%20Guide%20-%20v1.1.pdf | 2026-09-24 | `85dbb7b996737672bca4600a9649bb24fbf0f43dc1b0d90698693da5da45680c` |
| AB | Ableton Live 12 MIDI Remote Script `Oxygen_5th_Gen` (decompiled): `__init__.py`, `oxygen_5th_gen.py`, over `Oxygen_Pro` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Oxygen_5th_Gen | 2026-09-24 | `a03fdcd8…00cceb` (\_\_init\_\_.py), `1c2a32ae…b7b302` (oxygen_5th_gen.py); `Oxygen_Pro` as in `../m-audio-oxygen-pro/SOURCES.md` |
| MA | M-Audio, "Oxygen MKV Series – Setup Guide in Ableton Live", "… Setup in Pro Tools", "… Setup in FL Studio" (DAW modes, port names) | https://support.m-audio.com/en/support/solutions/articles/69000805953-m-audio-oxygen-mkv-series-setup-guide-in-ableton-live | 2026-09-24 | — (web pages) |

## Facts, one by one

| Fact | Value | Evidence | Source |
|---|---|---|---|
| DAW modes | NC1, NC2 (Mackie), M\|h (Mackie/HUI), N1 (MIDI, Ableton), N2 (MPC Beats, Reason), N3 (Ableton, clip launching) | Documented | UG p13–14 |
| Ableton's script for the MKV is the Oxygen Pro's | `Oxygen_5th_Gen(Oxygen_Pro)`, `LIVE_MODE_BYTE = 0`, no session component | Official software (one source) | AB |
| Its port is the second | two in, two out; the second pair is the script's | Official software (one source) | AB `__init__.py` |
| Port names | Windows `Oxygen 49 MKV`, `MIDIIN2 (Oxygen 49 MKV)`; Mac `Oxygen 49 DAW` (QS) or `Oxygen ## MKV Mackie/HUI` (MA), **which conflict** | Documented | QS; MA. The matcher accepts `midiin2`, `mackie` and `daw` |
| Mode messages | 6D 00, 6E 02, 6E 07 (and 6B 01, 6C 03, left out) | Official software (one source) | AB `on_identified` with `live_mode_byte = 0` |
| Knobs 1–8 | CC 22–29, channel 1, absolute | Official software (one source) | AB (inherited) |
| Faders 1–8, fader 9 (49, 61) | CC 12–19; CC 41, the master | Official software (one source) | AB (inherited); nine faders, UG p14 |
| Fader buttons 1–8 (49, 61) | CC 32–39; declared non-momentary | Official software (one source) | AB (inherited). No slot |
| < and > | scroll track banks in DAW mode; CC 110, 111 | Documented (function); **Assumption** that they are the script's Bank < > | UG p13; AB `Bank_Left/Right_Button` |
| Transport | Loop 114, Stop 117, Play 118, Record 119 | Documented (buttons, UG p14); Official software (one source) | AB (inherited) |
| Not declared: the pads | eight pads, two banks; notes not established in this mode | — | UG p15; AB `set_pad_translations` |
| Not declared: the 25's fader | — | Not documented | — |
| Identity Reply | M-Audio `00 01 05`, family LSB 00 | Official software (one source) | AB (inherited `product_id_bytes`) |
| Collision | an older Oxygen (fourth generation) named "Oxygen 49" with a second port would be claimed; none is documented | Assumption | — |

## Open questions, until someone with the hardware checks

1. **Whether the MKV answers the messages** and sends the controls above,
   with no DAW mode chosen by hand. M-Audio's Ableton article has the player
   choose N1 on the keyboard.
2. **The pads' notes** in this mode, and **the 25's fader**.
3. **The port names on a Mac and on Linux.**
