# RackForge Android prototype

This first Android APK displays Android low-latency capabilities and enumerates
USB, MIDI and audio devices. It intentionally ships without instrument plugins:
each `.rfplugin` is versioned and distributed by its own project. The app opens
with an empty store, supports portable package installation, and restores the
last compatible installed plugin automatically. Its `arm64-v8a` JNI library
loads the selected package with RackForge Core and Wasmtime, accepts USB MIDI
and renders a 48 kHz stereo stream from an AAudio low-latency callback.

The output selector follows Android device additions and removals and can open
the system default, built-in speaker, or a selected USB audio interface without
putting the WebView or Java/JNI allocations in the realtime path. AAudio tries
exclusive low-latency mode first and falls back to a shared stream when the
device does not expose an exclusive path. Low, balanced and safe modes tune the
AAudio buffer to two, three and four hardware bursts respectively; Settings
reports the actual burst size, buffer and xrun count. Portable Wasm rendering
runs on a dedicated audio-priority producer thread into a preallocated lock-free
queue. Low keeps 384 frames rendered ahead, while balanced and safe keep 1,152,
so plugin CPU spikes do not run inside or stall AAudio's realtime callback.

PLAY opens the active plugin surface directly. LIVE is a separate performance
overview, so installing or activating a package is no longer part of normal
sound navigation. USB MIDI device callbacks automatically close stale ports and
reconnect enabled inputs after hotplug without allocating a byte array for every
MIDI message. When an input disappears, RackForge releases sustain and sends
All Notes Off on all sixteen channels before reconnecting, matching the
Raspberry Pi supervisor and preventing notes held during unplug from droning.
For the certified KeyLab Essential mk3 profile Android also opens the USB MIDI
destination owned by the controller runtime, performs the shared DAW handshake,
applies the same dim RGB profile to every button and pad, and restores Arturia
mode when the route closes.

While audio is active, a media-playback foreground service keeps RackForge at
foreground process importance when the screen is locked or the activity is in
the background. AAudio itself owns the playback wake lock; RackForge does not
hold an additional CPU wake lock. The stream error callback triggers an
off-callback reopen path when Android disconnects an audio route.

Build from the repository root:

```powershell
.\tools\build-android.ps1
```

On Linux and in GitHub Actions:

```bash
bash tools/build-android.sh
```

The local toolchain is stored below ignored `local/android-toolchain`, and the
APK is written to `dist/android/RackForge-debug.apk`.

## Lifecycle qualification

The debug APK exposes a permission-protected ADB snapshot receiver that is not
present in release builds. After installing the APK and connecting through USB
or wireless debugging, run the full screen-lock, background/resume, MIDI and
USB-audio recovery scenario from the repository root:

```powershell
python tools/qualify-android-lifecycle.py --usb-cycle
```

Disconnect and reconnect the powered hub when prompted. The harness verifies
that audio callbacks keep advancing, MIDI ports reopen, and AAudio returns to
the actual selected USB device. It writes the complete device metadata,
counters and stage snapshots to `dist/qualification/` as JSON. A run without
`--usb-cycle` is useful as a quick lifecycle smoke test, but its USB stage is
reported as skipped and does not constitute full hardware qualification.

For sustained native-audio qualification, leave the debug APK open with an
instrument active and run:

```powershell
python tools/soak-android.py --duration-minutes 120
```

Use a smaller duration while developing. Runs shorter than two hours are
recorded as smoke tests rather than release qualifications. The manual
`Qualify Android hardware` workflow can execute the same command on a dedicated
self-hosted runner and retain its JSON report as a GitHub Actions artifact.
