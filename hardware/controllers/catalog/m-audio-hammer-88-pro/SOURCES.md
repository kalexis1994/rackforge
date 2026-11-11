# M-Audio Hammer 88 Pro — sources

The package describes the keyboard in the **DAW mode RackForge puts it in
when it connects**, as for the Oxygen Pro. No value was measured on hardware.

Both official scripts treat the Hammer 88 Pro as an Oxygen Pro:
- Ableton's `Hammer_88_Pro` script is `class Hammer_88_Pro(Oxygen_Pro): pass`.
- Bitwig's Oxygen Pro extension drives it with the configuration of the
  Oxygen Pro 49 and 61: eight faders, a master fader and scene buttons.

Every control, message and caveat of `../m-audio-oxygen-pro/SOURCES.md`
therefore applies here. This file adds only what is the Hammer's own.

## Documents

| Tag | Document | Where | Retrieved | sha256 |
|---|---|---|---|---|
| HUG | Hammer 88 Pro User Guide v1.3 | https://cdn.inmusicbrands.com/m-audio/maudio_documentation/Hammer%2088%20Pro%20-%20User%20Guide%20-%20v1.3.pdf | 2026-09-24 | `6772ce1eb8def28946c5993c35bfc4bbc42edf8b92f028435d5697c0660b2537` |
| HAB | Ableton Live 12 MIDI Remote Script `Hammer_88_Pro` (decompiled): `__init__.py`, `hammer_88_pro.py` | https://github.com/gluon/AbletonLive12_MIDIRemoteScripts/tree/main/Hammer_88_Pro | 2026-09-24 | `7b9c716a…f9a24f` (\_\_init\_\_.py), `f6600d25…855a67` (hammer_88_pro.py) |
| HBW | Bitwig, `OxygenPro88ExtensionDefinition.java` ("M-Audio Hammer 88 Pro") | https://github.com/bitwig/bitwig-extensions/tree/main/src/main/java/com/bitwig/extensions/controllers/maudio/oxygenpro | 2026-09-24 | `87867d22…55a77d` |
| HRS | M-Audio, "Hammer 88 Pro Series – Setup in Reason" (port names) | https://support.m-audio.com/en/support/solutions/articles/69000825035-m-audio-hammer-88-pro-series-setup-in-reason | 2026-09-24 | — (web page) |
| — | the Oxygen Pro's documents, tagged there UG, AB, BW | see `../m-audio-oxygen-pro/SOURCES.md` | | |

## Facts of its own

| Fact | Value | Evidence | Source |
|---|---|---|---|
| It runs the Oxygen Pro script | `Hammer_88_Pro(Oxygen_Pro)`, same ports (third in/out is the script's) | Official software | HAB |
| Bitwig drives it as an Oxygen Pro with faders and a master | `OxyConfig(8, true, true, true)`, the default of every Oxygen Pro definition the Hammer's does not override | Official software | HBW; BW `OxygenProExtensionDefinition.createInstance` |
| Ports | Windows: `Hammer 88 Pro`, `MIDIIN2 (Hammer 88 Pro)` (DIN), `MIDIIN3 (Hammer 88 Pro)` (Mackie/HUI); macOS: `Hammer 88 Pro, USB MIDI`, `…, MIDI DIN`, `…, Mackie/HUI` | Documented; Official software | HRS; HBW (`MIDIIN3/MIDIOUT3 (Hammer 88 Pro)` on Windows, `Hammer 88 Pro Mackie/HUI` on Mac and Linux) |
| Controls | 9 faders with 9 buttons, 8 knobs, 16 pads, an encoder, the two buttons beside the pads, transport | Documented | HUG (Features) |
| USB product | 0x003C (60), M-Audio 0x0763 | Official software | HAB `product_ids=[60]` |

Every value in `rackforge-controller.toml` is the Oxygen Pro's, with its
evidence. The same open questions apply, with one more: whether the Hammer's
firmware answers the Oxygen Pro's mode messages as the Oxygen Pro does. Both
scripts send them to it unchanged.
