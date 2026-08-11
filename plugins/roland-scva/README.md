# Roland SCVA reference plugin

Reference instrument plugin that plays a multisampled bank generated from a
legitimate Sound Canvas VA installation without bundling proprietary audio or
software.

The manifest declares the required `rendered-bank` directory. Core validates
the resource before loading plugin code and provides it through the host API.
Every WAV may be float32 or PCM16, mono or multichannel; the plugin downmixes to
mono and adapts sample rate during playback.

Implemented behavior includes:

- 16 voices with deterministic stealing;
- sample-accurate automation;
- persistent state;
- sustain and continuous release;
- per-instrument ADSR;
- volume, attack, decay, sustain, and release parameters;
- dynamic programs derived from the external bank.

The plugin is a compatibility/reference fixture. Distributable product
instruments live in independent repositories, and user-rendered banks stay
outside Git and release artifacts.
