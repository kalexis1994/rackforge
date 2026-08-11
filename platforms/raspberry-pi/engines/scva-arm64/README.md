# SCVA ARM64 bank research

`rackforge-scva-bank` is the first native ARM64 layer of the experimental
SC-8820 path. It reads wave candidates extracted from a legitimate Sound Canvas
VA installation; it does not contain or distribute those files.

The library:

- requires the expected files and exact sizes;
- validates each 1 MiB segment marker/date;
- hashes and identifies the analyzed 1.1.2 corpus;
- preserves segment boundaries and lookup tables;
- decodes bounded FCE-DPCM ranges to PCM for research;
- validates control tables by size and SHA-256;
- resolves tones and MIDI notes to partials, wave maps, and sample descriptors;
- renders native partials with ROM mapping, FIR interpolation, tuning, and
  envelopes;
- can preload the tested note range and stream stereo S32_LE at 48 kHz.

This remains an experimental engine, not the portable plugin distribution path.
User-derived banks and rendered audio stay outside Git.
