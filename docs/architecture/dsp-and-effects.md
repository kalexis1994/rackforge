# DSP and effects policy

Status: accepted for the native prototype.

## Separation of responsibilities

RackForge distinguishes three layers:

1. `rackforge-dsp` contains platform-neutral, real-time-safe signal-processing
   primitives.
2. An effect plugin exposes a reusable rack node through the RackForge plugin
   ABI.
3. An instrument plugin may use the same primitives for effects that belong to
   its private program format. RF-DLS does this for its program-wide FX chain.

The third case is not a replacement for rack effects. It lets an RF-DLS custom
program remain self-contained, while a future LIVE rack can still insert
independent chorus, reverb or other effect plugins before or after any
instrument.

Program documents store stable semantic settings, never delay buffers, sample
history, operating-system handles or implementation-specific pointers.

## Real-time rules

- allocate delay lines and work buffers during activation;
- never allocate, lock, log, access files or call a controller from `process`;
- use bounded buffers and finite parameter ranges;
- smooth live parameter changes in the DSP node;
- make reset deterministic;
- keep audio code independent of ALSA, CoreAudio, WASAPI and WebAssembly;
- test silence, reset, invalid parameters, finite output and bounded response.

These rules apply equally to native and future WebAssembly processors.

## Source and license policy

Bundled DSP should be original RackForge code or come from a dependency with a
reviewed MIT, BSD or Apache-2.0 license. Every imported implementation must pin
its source revision and preserve the required license notices. Search catalogs
such as OpenAudio are discovery tools, not proof that an implementation is safe
to redistribute.

The initial chorus is original Rust code using the conventional
LFO-modulated, interpolated-delay architecture. The design was compared with:

- DaisySP chorus, MIT:
  <https://github.com/electro-smith/DaisySP>
- Signalsmith DSP delay utilities and documentation, MIT:
  <https://signalsmith-audio.co.uk/code/dsp/>

No DaisySP source code is copied. DaisySP's `ReverbSc` is deliberately excluded
because it is distributed in the separate LGPL DaisySP-LGPL repository.

The first RackForge reverb is an original eight-line stereo feedback delay
network with a Householder feedback matrix, per-line T60 decay and one-pole
damping. CloudSeed, MIT, was used only as an architectural reference for
algorithmic reverberation; no source code was copied:

- <https://github.com/ValdemarOrn/CloudSeed>

Claims that old Freeverb copies are “public domain” are insufficient without a
traceable upstream source and license file.

## RF-DLS shared chain

The first chain is deliberately small and ordered:

```text
Layer A ─┐
         ├─ mix ─ chorus ─ reverb ─ program output
Layer B ─┘
```

Chorus parameters were introduced in payload version 3 and reverb parameters
in version 4. Payload versions 1 and 2 migrate with both effects disabled;
version 3 migrates with reverb disabled. Existing programs therefore remain
sonically unchanged until an effect is explicitly enabled.
