# Sound Canvas VA 1.1.2 reverse-engineering notes

Analysis date: July 29, 2026.

These notes describe a legitimate local installation. Binaries, extracted
banks, and test WAV files are not part of RackForge and must not be
redistributed.

## Analyzed binary

```text
file: SCCore.dll
size: 27,358,208 bytes
architecture: Windows PE x86-64
SHA-256: 0635cc2bfced7876694f362f29719bae58e4539d576af9321673f6ffc31f6735
```

Most wave data lives in `.rdata`. Internal markers identify four groups of
1 MiB segments:

| Extracted candidate | Offset | Size | SHA-256 |
| --- | ---: | ---: | --- |
| `wave_1994_ver200_8mib.bin` | `0x966c0` | 8 MiB | `05a36e2e354611e667b643d619c9c1d2a2f0836bd585189e061b82f27b827385` |
| `wave_1996_rom_make_a_8mib.bin` | `0x8966c0` | 8 MiB | `0e5edc077367165751464ee8d9028a5c6b23cf57ad69254d3ff687da5c2de0a6` |
| `wave_1996_rom_make_b_4mib.bin` | `0x10966f0` | 4 MiB | `bc96fb86fae38ce1b187e48b75e3bcbca444821522deb7b5105821759b51d391` |
| `wave_1999_sc8820_4mib.bin` | `0x14966f0` | 4 MiB | `5e7c4e32963da835db54e3663221606ee875bf1b20a0c4f0d57ebacdc5085be2` |

The complete region starts at `0x966c0`, spans `0x1800030` bytes including
the observed `0x30` inter-group gap, and has SHA-256
`437692123a2e5e2516eb9f3b2c90415719b8e31a66bfd0eb224bf2e79a6860e0`.

All 24 individual 1 MiB segments and adjacent 2 MiB pairs were compared with
known Nuked-SC55 SC-55mkII hashes. None matched, so these candidates are not
direct replacements for `waverom1.bin` and `waverom2.bin`.

## Synthesizer ABI

`SCCore.dll` exports a small C API including:

```text
TG_initialize
TG_activate
TG_deactivate
TG_setSampleRate
TG_setMaxBlockSize
TG_setInterruptThreadIdAtThisTime
TG_ShortMidiIn
TG_LongMidiIn
TG_Process
TG_XPgetCurTotalRunningVoices
```

`scva-render` signatures were cross-checked against kode54's public SCCore
host implementation:

<https://gist.github.com/kode54/01929e2f1dfc9ee4f8f1>

The probe loaded the DLL, selected Program 0, sent C4 at velocity 100, and
rendered four seconds at 44.1 kHz:

```text
frames: 176400
peak: 0.04777012
rms: 0.00322477
```

This proves that the ABI and internal data operate on Windows. It does not
imply ARM64 compatibility: the DLL contains x86-64 code and Windows
dependencies. Raspberry Pi support requires a native DSP/bank reader or a
translation layer whose latency must be measured.

## Native decoding

Each 1 MiB segment begins with a `0x8000`-byte scale table. For sample address
`a`, the scale byte is at `a >> 5`; bit 4 selects its low or high nibble.
The sample byte is a signed delta:

```text
pcm[n] = pcm[n - 1] + (signed_byte[a] << scale_nibble[a])
```

This matches the public Roland FCE-DPCM documentation/tool:

<https://gist.github.com/giulioz/39e96282371ffb5059e112f6281efa60>

The data inside SCCore is already ordered and requires no additional
descrambling. A bounded decoder output matched on Windows x86-64 and Debian
ARM64:

```text
group: sc88-rev200
segment: 0
range: 0x8000..0x18000
peak: 302076
WAV SHA-256: 52da487847336bb7839b82cfd9e349b49e9e1367015d3ded496c0013cc044958
```

## Control tables

A descriptor table in `.data` references six static control blocks:

| Block | Offset | Size | Confirmed structure |
| --- | ---: | ---: | --- |
| System | `0x189a5d0` | `0x58` | global configuration; `u16 +2 = 2` partials |
| Named addresses | `0x189a630` | `0x4b0` | 50 × `0x18` records |
| Sample descriptors | `0x189aae0` | `0x16e04` | 4,259 × `0x16` records plus 2 trailing bytes |
| Drum kits | `0x18b18f0` | `0x1bc20` | 88 × `0x50c` records |
| Wave maps | `0x18cd510` | `0x28294` | 1,175 × `0x8c` records |
| Tones | `0x18f57b0` | `0x93b00` | 2,363 × `0x100` records |

Each tone has a `0x24`-byte header and two contiguous `0x6e` partials. In
each partial, `u16 +2` selects a wave map and byte `+4` offsets the note
around `0x40`.

Each wave map contains:

```text
+0x00  12-byte name
+0x0c  32 upper key limits (u8)
+0x2c  32 sample descriptor indices (u16 LE)
+0x6c  32 per-zone parameters (u8)
```

For Piano 1 (tone 0) and C4 (note 60):

```text
partial 1: map 0 "Stway end"    -> zone 7 -> sample 7
partial 2: map 1 "Steinway-D p" -> zone 7 -> sample 20
```

## Sample descriptor

The DSP interprets each `0x16`-byte descriptor as:

- byte 0: flat Wave ROM segment (`0..23`);
- bytes 1..3, 7..9, and 11..13: three 20-bit addresses using only the low
  nibble of the first byte;
- byte 10: flags; bit 1 marks one-shot (clear means loop) and bit 2 reverses
  layout;
- `u16 LE +4` and byte `+6`: fine tuning and root key.

Flat segments map to extracted groups:

```text
0..7   -> wave_1994_ver200
8..15  -> wave_1996_rom_make_a
16..19 -> wave_1996_rom_make_b
20..23 -> wave_1999_sc8820
```

Piano 1/C4 descriptors:

```text
sample 7:
  rom-make-a segment 0
  start=0x74ee0 loop=0x7ed23 end=0x836de
  root=64 fine=972 flags=0

sample 20:
  rom-make-a segment 2
  start=0x290a0 loop=0x2e8db end=0x311df
  root=74 fine=1000 flags=0
```

`rackforge-scva-bank render-tone` resolves the chain, decodes FCE-DPCM,
applies root tuning, and mixes both partials. Its C4 preview matched on Windows
x86-64 and Debian ARM64:

```text
frames: 75054 at 32 kHz
SHA-256: 286b5a34530c6b87ac77e72c62b41cc50ea34a60a5906b8168bf24d6d303106f
```

## Runtime playback layout

Voice preparation aligns the first address to 32 bytes and keeps independent
data and scale bases:

```text
aligned_start = descriptor_start & ~0x1f
data_base     = aligned_start - 0x20
scale_base    = (aligned_start >> 5) - 0x20

delta[n]      = segment[data_base + n]
scale_byte[n] = segment[scale_base + (n >> 5)]
```

Using `(data_base + n) >> 5` is incorrect: it selects another scale-table
region and caused the harsh timbre heard in early native tests.

Dynamic validation of Strings 1, partial 2, block 16:

```text
descriptor start: 0x1ff22
aligned_start:    0x1ff20
data_base:        0x1ff00
scale_base:       0x0fd9
position:         278
SCCore accumulator (before <<10): -7447
RackForge accumulator:             -7447
```

Data and scale pointers were dumped separately and uniquely located inside the
ROM segment. Dumps and derived WAV files remain outside Git.

## Cursor and interpolation

The bank's nominal rate is 44.1 kHz. SCCore uses a 16-bit phase accumulator;
the upper seven fractional bits select one of 128 FIR phases. Four coefficients
apply to:

```text
pcm[position - 3], pcm[position - 2], pcm[position - 1], pcm[position]
```

The decoder primes four samples, so the initial interpolator cursor is 3.
Observed Strings 1/C4 increments at 44.1 kHz:

```text
partial 1: 45774 / 65536 = 0.698455810546875
partial 2: 56467 / 65536 = 0.8616180419921875
```

After correcting ROM mapping, a 32-frame partial-2 block, with observed TVA
gain removed, correlated with the ARM64 implementation at
`0.9999999999999991`.

## Loop continuity

Looped decoding preserves both the DPCM accumulator and FIR phase from `end`
back to `loop_start`. Strings 1/C4 deltas close exactly:

```text
partial 1: accumulator(end) - accumulator(before_loop) = 0
partial 2: accumulator(end) - accumulator(before_loop) = 0
```

The first ARM64 click came from rounding `loop_start / increment` to an
integer frame in the 48 kHz cache, which returned with a different fractional
phase.

Until decoding becomes fully incremental per voice, the native cache uses a
192-frame (4 ms) linear bridge and resumes after the head consumed by the
crossfade. On Strings C2 partial 1, the join discontinuity fell from 5,224 to
155 unnormalized PCM units.

## TVF

Normal mode uses a state-variable low-pass:

```text
s2 += cutoff * s1
output = s2
s1 += cutoff * (input - (resonance * s1 + s2))
```

Observed Strings 1/C4 coefficients were `cutoff=1` and `resonance=1`; the
TVF was open. Broken audio originated earlier in data/scale mapping and playback
rate.

## Reverb and perceptible repetition

A sustained seven-second Strings 1/C4 capture begins almost mono. During
sustain, stereo difference grows to roughly 8–17% of center level and changes
over time. SCCore overlays a decorrelated temporal tail that prevents exact
block repetition.

Control captures identify that tail primarily as reverb:

```text
CC91=0, CC93=0: nearly dry and mono
CC91=0:         nearly identical to the dry capture
CC93=0:         preserves most default stereo opening
```

Default-minus-dry sustain RMS measured 0.00187 left and 0.00372 right, versus
approximately 0.0164 dry RMS. `scva-render` supports `--duration-ms`,
`--note-off-ms`, and repeated `--cc NUMBER VALUE` pairs.

RackForge's first native stage uses a stable stereo comb/all-pass network at
48 kHz with restrained return. It is a provisional layer separate from the
decoder while SCCore topology and coefficients are identified.

## Remaining work

The ARM64 reader now reproduces waves with verified layout, interpolation,
tuning, and loops. Exact TVA/TVF envelopes, velocity/volume curves, modulation,
effects, and remaining descriptor modes are still required before it can be
considered a complete SCCore reproduction.
