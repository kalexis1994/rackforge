# asio-sys, RackForge fork

This is `asio-sys` 0.2.6 from crates.io, the ASIO layer cpal's ASIO host is
built on, with the changes below. The workspace points cpal at it through
`[patch.crates-io]` in the root `Cargo.toml`. Upstream:
<https://github.com/RustAudio/cpal/tree/master/asio-sys>. License: Apache-2.0,
see `LICENSE`. The version is `0.2.6+rackforge.1` so the lockfile and
`cargo tree` show which one is built.

## Why

A host that times its own callbacks only sees its own lateness. A driver can
lose a buffer while still calling the host on time (a USB transfer that missed
its slot, a DPC from another driver holding the CPU), and then the host's load
is low, it has no overruns, and the audio clicks anyway. ASIO has two messages
for the driver to report this, and upstream 0.2.6 drops both of them:

- `kAsioOverload` is not declared as supported, so a driver never sends it,
  and it falls into the `_ => 0` arm if one does. JUCE counts exactly this
  message as its ASIO xrun.
- `kAsioResyncRequest` ("the driver encountered some non fatal data loss")
  is acknowledged and marked `TODO: Handle this`. RtAudio reports it as an
  output underflow.

Two more things the upstream crate leaves out:

- `kAsioResetRequest` is forwarded only to message callbacks, and cpal
  registers none. When a driver asks to be reset, because its buffer size or
  sample rate was changed in its own window, nobody hears it.
  `kAsioBufferSizeChange`, which JUCE handles as a reset, is not declared
  at all.
- `ASIOControlPanel`, the SDK call that shows the driver's own settings
  window, is not in the bindgen allowlist.

## What changed

Almost all of it is in `src/bindings/mod.rs`, and each change is marked
there:

- `kAsioOverload` is declared supported and counted.
- `kAsioResyncRequest` is counted. The stream is left running, as RtAudio
  leaves it, since the loss has already happened.
- Each buffer switch's sample position is compared with the previous one. A
  step of more than one buffer counts the buffers it skipped. This catches
  drivers that lose audio without sending either message. The chain resets
  on `ASIOStart`, on `ASIOCreateBuffers`, and on any switch that reports no
  valid position.
- `driver_dropouts()` returns the three counts as `DriverDropouts`. They are
  process-wide totals and they overlap, since a driver may report one loss in
  more than one way.
- `kAsioResetRequest` and `kAsioBufferSizeChange` are counted, and
  `driver_reset_requests()` returns the count. The host compares it with the
  count it saw when it opened its stream, and reopens the stream when the
  count has grown.
- `open_control_panel()` calls `ASIOControlPanel`. It has to be called on the
  thread that loaded the driver.

`build.rs` adds `ASIOControlPanel` to the allowlist and now generates the
bindings every time it runs. Upstream skipped generation whenever
`asio_bindings.rs` already existed in `OUT_DIR`. That directory survives
changes to `build.rs`, so an allowlist change never reached a build that
already had bindings, including a cached CI `target/`.
`asio_stub_bindings.rs`, used on docs.rs, gains the matching stub.

Nothing else changed. The callback path gains one atomic swap per buffer
switch.

## Tests

The workspace excludes this crate so that its lints stay upstream's.
Run its tests on their own:

```text
cargo test --offline --manifest-path vendor/asio-sys/Cargo.toml --lib --target-dir target/asio-sys-fork
```

## Moving to a new upstream

If cpal moves to a newer `asio-sys`, check whether upstream now handles both
messages. If it does, delete this directory and the `[patch.crates-io]` entry.
If it does not, copy the new release over this directory and apply the change
again.
