# Raspberry Pi baseline

Verified reference system on July 29, 2026:

- Raspberry Pi 4 Model B.
- 64-bit Raspberry Pi OS Lite / Debian 13.
- Linux AArch64.
- Installation user selected at install time.
- Runtime root: `$HOME/rackforge/current`.

The service files are templates until the installer places and enables them.
No repository path or username is assumed.

## Verified peripherals

With the KeyLab and audio interface connected simultaneously:

- Arturia KeyLab Essential 61 mk3 MIDI and DAW endpoints enumerate correctly.
- Focusrite Scarlett Solo enumerates at USB high speed.
- Power status reports `throttled=0x0`.
- The Scarlett exposes stereo S32_LE playback/capture from 44.1 to 192 kHz.

The reproducible low-latency starting profile lives in `../audio/`. Device
selection uses stable identity and never persists an ephemeral ALSA card index.
