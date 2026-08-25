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

## Qualification still required for v0.2.0

- Exercise disconnect and reconnect sequences against the trace runner while
  proving stable identity and held-note release.
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
