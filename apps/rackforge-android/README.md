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
device does not expose an exclusive path.

Build from the repository root:

```powershell
.\tools\build-android.ps1
```

The local toolchain is stored below ignored `local/android-toolchain`, and the
APK is written to `dist/android/RackForge-debug.apk`.
