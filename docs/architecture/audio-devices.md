# Audio device management

Status: playback and capture inventory, stable selection, validated activation,
and mono/stereo plugin routing implemented; schema `1`

## Boundary

Plugins process RackForge audio buses. They never receive ALSA card numbers,
USB paths or permissions to configure physical devices. The host owns device
discovery, selection, negotiation, recovery and channel mapping.

`rackforge-audio-api` contains portable data contracts only. The initial Linux
provider lives in Core and uses ALSA `hw` endpoints to avoid hidden resampling,
mixing and latency. Other platform backends can produce the same descriptors and
consume the same output profiles.

Capture is deliberately opt-in. A newly connected microphone or interface is
never monitored automatically: the user must select the device and one or two
ordered physical channels. This prevents accidental room monitoring and
feedback. Disconnecting a selected interface leaves its stable selection
pending; RackForge never substitutes an unrelated microphone silently.

## Identity and selection

ALSA card indexes are ephemeral and must never be persisted. RackForge creates
stable IDs in this order:

1. USB vendor, product and serial plus the PCM device number;
2. USB vendor, product and physical USB path when no serial exists;
3. stable ALSA card ID plus the PCM device number for built-in devices.

Startup profiles select either an exact RackForge device ID or a typed USB
identity. A USB selector without a serial is accepted only when it matches one
device. Multiple matches are an error, never an arbitrary first choice.

The default fallback is `none`. The optional `unique_compatible` policy may be
used only when exactly one discovered output supports the complete requested
profile. It never chooses between multiple candidates.

## Transactional activation

Core performs these steps before exposing an output to the real-time loop:

1. validate the persisted profile structurally;
2. inventory cards and isolate failures per advertised stream;
3. resolve an unambiguous stable selector;
4. validate format, rate, channels, period and buffer against discovered
   capabilities;
5. apply one ALSA `hw_params` transaction;
6. read back and compare every negotiated value;
7. prepare the PCM stream.

Any failure closes the candidate without publishing `AUDIO_READY`. An unplugged
active device makes the audio process exit; systemd restarts Core, which resolves
the stable identity again even if the ALSA card index changed. The session
checkpoint restores the stable PLAY/LIVE context.

Input activation follows the same transaction. Its rate and period must match
the output so a full-duplex interface shares one clock domain. Physical channel
numbers are one-based and ordered: `[2]` maps interface input 2 to plugin input
1, while `[2, 1]` swaps a stereo pair. Input trim is bounded to -60..+24 dB and
is applied before plugin processing with non-finite values sanitized.

Desktop capture and playback callbacks exchange complete frames through a
bounded single-producer/single-consumer ring. The callbacks never allocate,
lock, wait, touch files, or log. Pressure drops complete input frames rather
than corrupting stereo alignment, and exposes overrun/underrun counters in
audio diagnostics.

Disconnected HDMI endpoints are a normal condition. A driver failure while
probing one advertised stream is diagnosed as `AUDIO_STREAM_IGNORED` and does
not discard other devices.

## Current renderer constraints

The device API can describe multiple formats and channel ranges. The Linux
renderer currently accepts:

- interleaved `S32_LE` device samples;
- stereo output;
- one or two selected capture channels;
- configurable sample rate, period and buffer.

Desktop uses the formats exposed by CPAL/WASAPI/ASIO and maps one or two
selected physical inputs to a plugin's declared mono/stereo main bus. Mono
capture is duplicated for a stereo effect; stereo capture is averaged for a
mono effect. No input selection produces deterministic silence.

This matches the current Scarlett path without pretending that unimplemented
converters or layouts are safe. Unsupported renderer profiles fail explicitly.

At 48 kHz, period 128 and buffer 384, nominal device buffering is 8 ms. This is
not a round-trip latency claim; USB scheduling, DSP and monitoring add their own
latency.

## Rack and plugin routing

Plugin manifests may declare explicit named audio buses. Legacy instruments and
effects without the declaration retain their historical stereo main bus. Rack
graphs can route `Audio Input -> Effect -> Effect -> Audio Output`, or mix
instrument outputs into downstream effects. The compiler topologically orders
dependent nodes; independent instrument-only racks keep the parallel renderer.

Every plugin process call receives normalized interleaved buffers. Plugins do
not know whether samples originated in ALSA, ASIO, WASAPI, a DAW, or a future
Android full-duplex backend.

## Deferred device controls

Air, direct monitoring, line/instrument mode and phantom power belong to
optional device-control extensions. Raw ALSA mixer controls will not be exposed
to plugins or generic UI. Until a typed vendor extension exists, instrument/line
mode and hardware gain remain settings on the interface itself.

Phantom power is classified as a hazardous external-power action. A future
typed control must require explicit confirmation and an explicit persistence
policy. It must never be enabled by generic fallback or automatic device
restoration.
