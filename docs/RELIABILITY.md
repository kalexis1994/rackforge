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

## Qualification still required for v0.2.0

- Inject queue saturation, stream loss, underruns, and recovery into the native
  audio hosts without adding work to their callbacks.
- Add a repeatable ADB scenario for screen lock, background, resume, USB MIDI,
  and USB audio recovery.
- Add a native soak command that runs for at least two hours and exports xruns,
  dropped MIDI, missed deadlines, reconnects, and stream errors.
- Record supported test hardware and pass/fail thresholds with every retained
  qualification report.

The live checklist and acceptance criteria are tracked in
[GitHub issue #12](https://github.com/kalexis1994/rackforge/issues/12).
