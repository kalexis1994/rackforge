# RackForge Android prototype

This first Android APK displays Android low-latency capabilities, enumerates
USB, MIDI and audio devices, and packages the exact portable RF-XP10
`.rfplugin` used by Windows and Raspberry Pi. Its `arm64-v8a` JNI library loads
that package with RackForge Core and Wasmtime, accepts USB MIDI and renders a
48 kHz stereo stream from an AAudio low-latency callback. The screen is
refreshed whenever the activity resumes.

The output selector follows Android device additions and removals and can open
the system default, built-in speaker, or a selected USB audio interface without
putting the WebView or Java/JNI allocations in the realtime path. AAudio tries
exclusive low-latency mode first and falls back to a shared stream when the
device does not expose an exclusive path. Low, balanced and safe modes tune the
AAudio buffer to two, three and four hardware bursts respectively; Settings
reports the actual burst size, buffer and xrun count.

PLAY opens the active plugin surface directly. LIVE is a separate performance
overview, so installing or activating a package is no longer part of normal
sound navigation. USB MIDI device callbacks automatically close stale ports and
reconnect enabled inputs after hotplug without allocating a byte array for every
MIDI message. When an input disappears, RackForge releases sustain and sends
All Notes Off on all sixteen channels before reconnecting, matching the
Raspberry Pi supervisor and preventing notes held during unplug from droning.

While audio is active, a media-playback foreground service keeps RackForge at
foreground process importance when the screen is locked or the activity is in
the background. AAudio itself owns the playback wake lock; RackForge does not
hold an additional CPU wake lock. The stream error callback triggers an
off-callback reopen path when Android disconnects an audio route.

Build from the repository root:

```powershell
.\tools\build-android.ps1
```

The local toolchain is stored below ignored `local/android-toolchain`, and the
APK is written to `dist/android/RackForge-debug.apk`.
