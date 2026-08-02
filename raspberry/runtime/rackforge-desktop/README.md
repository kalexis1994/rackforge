# RackForge Desktop

The Desktop host is the portable development shell for RackForge. It embeds
the real LITTLE menu runtime in a native window, exposes the existing Web SPA
and maps four virtual buttons to the controller-independent input contract.

This first stage deliberately does not emulate an Arturia device or send
SysEx. It lets RackForge surface behavior be developed and tested without the
Raspberry Pi or a physical controller.

## Input

| Action | Mouse | Keyboard |
|---|---|---|
| Button 1–4 short press | Click | `Q`, `W`, `E`, `R` |
| Button 1–4 long press | Hold for 700 ms | Hold `Q`, `W`, `E`, `R` |
| OK / previous / next / back | Current footer buttons | `Enter` / left / right / `Escape` |

The labels come from the rendered LITTLE footer; they are not hard-coded into
the desktop controls.

## Web server

The embedded Web UI listens on `127.0.0.1:8787` by default. Use:

```text
rackforge-desktop.exe --port 9000
rackforge-desktop.exe --lan --port 8787
```

`--lan` binds to all interfaces. Local Desktop sessions currently trust the
local user and do not require device pairing.

## Build for Windows x86-64

From the repository root, run:

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-windows-desktop.ps1
```

The script uses the stable MSVC Rust target and Visual Studio Build Tools, then
writes a portable release executable to
`dist/windows-x86_64/RackForge.exe`.

The desktop host is an integration stage, not a separate UI implementation.
Native plugin execution and Windows MIDI/audio backends are the next platform
layers; the LITTLE menu and Web application remain shared with Raspberry Pi.
