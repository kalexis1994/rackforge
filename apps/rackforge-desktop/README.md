# RackForge Desktop

The Desktop host runs RackForge Core and embeds the existing React Web
workspace in its native Windows window through Wry and WebView2. Rust remains
the application host, plugin runtime and local Web/API server; Desktop does
not maintain a second UI implementation.

The application menu is native Win32 UI, themed to match RackForge, and stays
above the embedded workspace. **File → Install Plugin…** opens the Windows
file picker, **Settings → Settings…** opens the desktop configuration area,
and **View** provides reload, external-browser and debug tools.

Desktop permits one RackForge process across Windows sessions. A second launch
shows an informational dialog and exits before configuration, MIDI, or the
audio engine is initialized. Windows releases the named mutex automatically if
the owning process exits or crashes.

The active mode, plugin instance, selected program, master level, and master
pan are checkpointed under `data/sessions/live.main.json`. Desktop restores
and validates that context before starting audio, then synchronizes LITTLE so
a long BACK press returns to the sound that is actually playing. Missing
plugins or programs fall back safely without discarding the remaining session.

## Windows audio and MIDI

Desktop opens the saved Windows output through WASAPI, or the system default
on first start, and connects the selected MIDI inputs. MIDI is delivered
directly to the active plugin on the audio callback; plugin and preset changes
cross a bounded command queue instead of sharing the realtime instance with
the UI.

Use **Settings → Settings… → Audio & MIDI** to choose the driver, output device, sample rate,
buffer size, Desktop output gain and MIDI inputs. Desktop defaults to +6 dB to
compensate for quiet plugin output, with selectable values up to +12 dB and
clamping at the final device boundary. Changes are applied by safely reopening the
stream and are persisted in `config/audio.toml` below the RackForge Root. If a
new configuration fails, Desktop attempts to restore the previous stream.
The panel also provides device rescanning and an audio test note. Release builds
include WASAPI and ASIO; every registered ASIO driver is enumerated separately.
ASIO is shown as unavailable only when Windows has no usable ASIO driver.
**View → Audio & MIDI Status** reports the active stream.

## Controller surfaces

LITTLE is a surface for external controllers. Its `egui` simulator remains
available in the source as a development harness, but it is not the Desktop
product UI.

## HTTP server

The embedded Web UI uses a private loopback connection chosen at startup. The
network HTTP server is disabled by default and is configured under
**Settings → Settings… → HTTP Server**. Its configuration is persisted in
`config/web.toml` below the RackForge Root.

For command-line launches, `--lan` enables the network server and `--port`
selects its port:

```text
rackforge-desktop.exe --port 9000
rackforge-desktop.exe --lan --port 8787
```

The network server binds to all interfaces. Local Desktop sessions currently
trust the local user and do not require device pairing, so network publishing
should only be enabled on a trusted network.

## RackForge Root and plugin installation

On first start, Desktop asks where RackForge should keep its library:

- **Recommended** uses `%LOCALAPPDATA%\RackForge` and is suitable for an
  installed application.
- **Portable** uses `RackForgeData` beside `RackForge.exe` and writes a
  `rackforge-portable.toml` marker beside the executable.
- **Custom** stores the library on any user-selected disk or directory.

All user-owned state stays below that single RackForge Root:

```text
RackForge Root/
├── plugin-store/
│   ├── packages/<plugin-id>/<version>/
│   └── records/
├── plugins/          # legacy unpacked packages remain supported
├── data/plugins/     # private plugin data
├── config/
├── sessions/
├── state/
├── cache/
└── logs/
```

Use **Plugin Manager → Install plugin** to choose a package. RackForge checks the file
type and size, extracts it into an isolated staging directory, validates its
manifest and complete payload, and then shows the plugin identity, target,
size and SHA-256 digest before asking for confirmation. Local packages are not
publisher-signed, so native packages also display an explicit trust warning.

After confirmation, Desktop installs the exact bytes that were inspected. The
commit is atomic, versions are immutable and kept side by side, and incomplete
files are rolled back. A newly installed plugin remains inactive until the
user activates it from Plugin Manager or chooses to open it after installation.
Activation and installation are separate, persistent states, so an incompatible
runtime cannot silently replace the instrument currently in use. Updating a
native plugin that is already loaded may still require a RackForge restart,
which avoids unloading or replacing native code while it is in use.
The same portable WASM `.rfplugin` can be copied between Windows and Raspberry
Pi; RackForge itself supplies the platform-specific host runtime.

The equivalent command-line flow is useful for automation and future file
associations:

```text
RackForge.exe --rackforge-root D:\RackForge --install-plugin D:\Downloads\rf-kr106.rfplugin
```

`--rackforge-root` bypasses first-start selection for that launch.
`--plugins-root` and `--data-root` remain available for older development
workflows, but local archive installation requires a RackForge Root.

## Build for Windows x86-64

From the repository root, run:

```powershell
powershell -ExecutionPolicy Bypass -File tools/build-windows-desktop.ps1
```

The script uses the stable MSVC Rust target and Visual Studio Build Tools. Both
release builds and builds invoked with `-Configuration Debug` write the latest
desktop executable to the single canonical path
`dist/windows-x86_64/rackforge.exe`.

The Desktop host is a platform shell around the shared RackForge Web
application. Windows MIDI/audio backends remain platform layers; controller
surfaces such as LITTLE continue to be shared with Raspberry Pi hardware.
