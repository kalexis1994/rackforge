# RackForge DSP

`rackforge-dsp` contains portable audio primitives shared by RackForge plugins.
It has no operating-system, controller, audio-driver, serialization or plugin
ABI dependencies.

## Real-time invariants

- memory is allocated only while constructing or preparing a processor;
- `process` and parameter updates do not allocate, lock, log or perform I/O;
- externally supplied values are validated before reaching the audio thread;
- discontinuous parameters are smoothed inside the processor;
- `reset` clears all history deterministically.

## Provenance

The chorus is an original Rust implementation of the conventional
LFO-modulated fractional-delay topology. The implementation was reviewed
against the MIT-licensed DaisySP chorus design and the MIT-licensed
Signalsmith DSP delay documentation; no source code was copied.

The ROOM reverb is an original Rust implementation built around an eight-line
stereo feedback delay network, a Householder feedback matrix, per-line T60
decay and one-pole damping. CloudSeed was reviewed only as an architectural
reference for algorithmic reverberation; no source code was copied.

The exciter is an original Rust implementation of the parallel topology
documented for the classic Korg M1: pre-EQ, unprocessed/excited blend, a
ten-position emphatic point and an outer dry/effect balance. RackForge's
nonlinear branch adds controlled presence and a biased harmonic residual at
2x. No Korg source code or proprietary DSP data is used.

- DaisySP: <https://github.com/electro-smith/DaisySP>
- Signalsmith DSP: <https://signalsmith-audio.co.uk/code/dsp/>
- CloudSeed: <https://github.com/ValdemarOrn/CloudSeed>
- Korg M1 Owner's Manual, effect parameters page 49:
  <https://cdn.korg.com/us/support/download/files/e223761dc7dee38a7c3c76983ddf2ea1.pdf>

RackForge does not use DaisySP's `ReverbSc`: that module is distributed
separately under the LGPL.
