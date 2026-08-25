# Reliability qualification

RackForge treats real-time reliability as a release qualification, not as a
subjective listening test. The `v0.2.0` milestone tracks five complementary
layers: deterministic MIDI replay, device hotplug, audio recovery, Android
lifecycle, and long-running native-host soak tests.

## Deterministic MIDI traces

`rackforge_core::midi_trace` defines the first qualification layer. A trace is
portable JSON and identifies inputs by their stable RackForge MIDI identity.
Runtime-only source keys and display names are never persisted.

```json
{
  "schema_version": 1,
  "events": [
    {
      "frame": 0,
      "source_id": "usb.arturia.keylab-essential-mk3",
      "message": [144, 60, 110]
    },
    {
      "frame": 127,
      "source_id": "usb.arturia.keylab-essential-mk3",
      "message": [128, 60, 0]
    }
  ]
}
```

Compilation is atomic. Before replay begins, RackForge verifies the schema,
monotonic frame order, stable source identity, MIDI status, message length, and
data-byte ranges. Events at the same frame retain their file order. The replay
visitor owns pacing, which lets unit tests run without sleeping and lets a
future soak runner use the same trace against a real audio clock.

Run the portable trace coverage with:

```text
cargo test -p rackforge-core midi_trace
```

The suite currently covers dense chords, Control Change, Pitch Bend, Channel
Pressure, Poly Pressure, stable same-frame ordering, malformed traces, unknown
devices, and delivery of every Note Off through the normal MIDI route.

## MIDI disconnect and reconnect

The platform-neutral `SupervisedMidiSources` state records a connection only
after opening the external port succeeds. Failed opens remain pending for the
next scan. A successful disconnect injects source-aware sustain release and
All Notes Off messages through the ordinary MIDI ingress and routing path.

Automated scenarios verify that sustained and physically held notes stop before
reconnection, that an ALSA client/port address change retains the same stable
identity and compiled route key, and that 10,000 unplug/replug cycles do not
require a host restart or produce duplicate transitions. Ambiguous source keys
or identities are rejected before the supervisor thread starts.

Run this layer with:

```text
cargo test -p rackforge-core midi_hotplug
```

## Audio fault injection and recovery

`rackforge_core::audio_reliability` owns the bounded stereo render queue and
the dropout/stream recovery state used by Android. The queue allocates its
complete ring during construction. Push, pop, concealment, recovery, and
telemetry use only preallocated memory and atomics after startup, so fault
handling adds no allocation, mutex, channel, sleep, or system call to the audio
callback.

The deterministic suite fills the queue to saturation, verifies that rejected
writes cannot expose partial stereo frames, forces partial and empty reads,
fades the last valid sample to silence, records a stream loss, and verifies a
finite click-reduced fade-in after restart. Android exposes the same counters
through its native audio status: saturated pushes, underrun callbacks and
frames, concealed/recovered callbacks, stream health, losses, and recoveries.

Run this layer with:

```text
cargo test -p rackforge-core audio_reliability
```

## Android lifecycle and USB recovery

`tools/qualify-android-lifecycle.py` drives a debug APK through ADB and records
machine-readable snapshots from a debug-only receiver. It proves that native
audio callbacks continue while the screen is locked and while the Activity is
in the background, then verifies clean resume transitions. The full hardware
mode also waits for a physical USB disconnect and reconnect, requires MIDI
ports to reopen under a newer generation, and compares AAudio's real device ID
with the restored selected interface. This catches a UI that claims to have
returned to USB while the stream still uses the fallback output.

With one authorized device connected, run the complete scenario from the
repository root:

```text
python tools/qualify-android-lifecycle.py --usb-cycle
```

The operator disconnects and reconnects the hub when prompted. Use `--serial`
when multiple ADB devices are online. Without `--usb-cycle`, the harness runs a
short lock/background smoke test and explicitly records the USB stage as
skipped. Every run writes a timestamped JSON report below
`dist/qualification/`; only a report whose top-level outcome is `passed` is a
valid qualification result.

## Native Android soak

`tools/soak-android.py` keeps the normal plugin and audio engine active by
injecting short notes through RackForge's MIDI ingress while sampling the
debug qualification snapshot. Duration, MIDI cadence, and sampling cadence are
configurable; a release qualification requires at least 120 uninterrupted
minutes:

```text
python tools/soak-android.py --duration-minutes 120
```

A shorter run is intended for development and receives the explicit outcome
`passed_with_duration_waiver`. The report accumulates counters across native
stream restarts rather than allowing a reset to hide a fault. It exports AAudio
xruns, render-queue underruns, callback deadline misses, MIDI drops, MIDI
reconnect attempts, disconnect panics, stream losses/recoveries, render errors,
lock misses, non-finite samples, callback stalls, process restarts, callback
load, thermal state, and total PSS memory.

The initial release thresholds are deliberately strict:

- zero xruns, queue underruns, missed deadlines, MIDI drops, render errors,
  lock misses, stream losses/recoveries, invalid samples, callback stalls,
  process restarts, snapshot failures, or unhealthy samples;
- maximum measured callback load of 85%;
- maximum Android thermal status of `MODERATE` (`2`).

Memory and reconnect-attempt counts are retained for trend comparison but do
not yet have a device-independent limit. The supported Android qualification
setup is an ARM64 phone on API 26 or newer, a powered USB hub, an Arturia KeyLab
Essential mk3, and a Focusrite Scarlett USB interface. The report records the
exact phone, Android build, attached USB identities, and runtime version; a
hardware result without that metadata is not accepted.

The manual `Qualify Android hardware` GitHub workflow targets a dedicated
self-hosted Windows runner labelled `rackforge-android`. It does not run on
ordinary pushes. The runner must have Python, ADB, an authorized phone, the
Android build toolchain, and the test hardware connected. The workflow builds
and installs the exact Git commit before testing it, embeds that revision in the
report, and retains the JSON as a 30-day Actions artifact even when a threshold
fails.

## Qualification still required for v0.2.0

- Record supported test hardware and pass/fail thresholds with every retained
  qualification report.

The live checklist and acceptance criteria are tracked in
[GitHub issue #12](https://github.com/kalexis1994/rackforge/issues/12).
