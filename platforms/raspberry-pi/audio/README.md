# Raspberry Pi audio reference

The verified primary output is a USB Focusrite Scarlett Solo 3rd Gen.

Reference capabilities:

- stereo playback;
- 24-bit samples transported as S32_LE;
- 44.1–192 kHz rates;
- USB high-speed transport.

ALSA card numbers are ephemeral and must never be persisted. The initial profile
uses a stable ALSA/USB selector and confirms the expected device identity before
opening it.

`rackforge-audio.toml` starts at 48 kHz, stereo S32_LE, 256 frames per period,
and a 768-frame buffer: RackForge's default buffer on every platform. A
continuous silence stream was stable at 128/384 without xruns or undervoltage
on the reference Pi, so 256 leaves headroom; LITTLE and the web settings offer
128, 256 and 512. It is a baseline, not a guarantee for every interface.

Use `probe.sh` to inventory the current device and negotiation capabilities.
