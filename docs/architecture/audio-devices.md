# Audio device management

Status: playback inventory, stable selection and validated ALSA activation
implemented; schema `1`

## Boundary

Plugins process RackForge audio buses. They never receive ALSA card numbers,
USB paths or permissions to configure physical devices. The host owns device
discovery, selection, negotiation, recovery and channel mapping.

`rackforge-audio-api` contains portable data contracts only. The initial Linux
provider lives in Core and uses ALSA `hw` endpoints to avoid hidden resampling,
mixing and latency. Other platform backends can produce the same descriptors and
consume the same output profiles.

The first implementation deliberately focuses on playback. Capture capabilities
are inventoried because they are intrinsic to a device, but capture routing and
vendor controls are not enabled yet.

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

Disconnected HDMI endpoints are a normal condition. A driver failure while
probing one advertised stream is diagnosed as `AUDIO_STREAM_IGNORED` and does
not discard other devices.

## Current renderer constraints

The device API can describe multiple formats and channel ranges, but the first
RackForge renderer intentionally accepts only:

- interleaved `S32_LE` device samples;
- stereo output;
- configurable sample rate, period and buffer.

This matches the current Scarlett path without pretending that unimplemented
converters or layouts are safe. Unsupported renderer profiles fail explicitly.

At 48 kHz, period 128 and buffer 384, nominal device buffering is 8 ms. This is
not a round-trip latency claim; USB scheduling, DSP and monitoring add their own
latency.

## Deferred capabilities

Capture, Air, direct monitoring, line/instrument mode and phantom power belong
to optional device-control extensions. Raw ALSA mixer controls will not be
exposed to plugins or generic UI.

Phantom power is classified as a hazardous external-power action. A future
typed control must require explicit confirmation and an explicit persistence
policy. It must never be enabled by generic fallback or automatic device
restoration.
