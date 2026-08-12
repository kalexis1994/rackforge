# RackForge

<p align="center">
  <img src="assets/brand/rackforge-logo.svg" width="320" alt="RackForge logo" />
</p>

RackForge turns MIDI controllers and general-purpose or embedded computers
into self-contained musical instruments, without requiring a desktop, monitor,
or DAW. The KeyLab Essential 61 mk3 paired with a Raspberry Pi 4B is the first
reference implementation, not an architectural limitation.

The cross-platform direction, portable plugin runtime, SDK, and future control
surfaces are described in the [technical roadmap](ROADMAP.md).

The repository separates the portable product from its platform adapters:

| Area         | Responsibility                                                            |
| ------------ | ------------------------------------------------------------------------- |
| `crates/`    | Core, APIs, SDK, and portable plugin runtime.                             |
| `apps/`      | Windows Desktop, Android, and headless/Web hosts.                          |
| `platforms/` | Platform-specific integrations, starting with Raspberry Pi.               |
| `hardware/`  | Drivers and packages for supported MIDI controllers.                      |
| `plugins/`   | Minimal conformance fixtures; instruments live in their own repositories. |
| `web/`       | RackForge's adaptive SPA.                                                 |

## Signal flow

```text
MIDI controller
  • keys, pads, encoders, buttons
           │ USB MIDI
           ▼
Platform MIDI + .rfcontroller package
  • discovers and reconnects hardware
  • maps controls to host intents
  • renders LITTLE, LEDs and pads
           │
           ▼
RackForge Core
  • authoritative musical state
  • portable plugins and programs
  • Racks, Songs and Setlists
  • routing, mixing and audio
           │
           ▼
Platform audio
  • ALSA / WASAPI / ASIO / AAudio
           │
           ▼
Built-in audio / USB interface
```

The host owns the authoritative engine, bank, and performance state. The
controller sends physical events, and its driver renders the state it receives.
If either side restarts, a handshake rebuilds the control surface without
depending on implicit state.

## Raspberry Pi development

Development helpers for connection, synchronization and diagnostics live in
`platforms/raspberry-pi/dev/`. Copy the provided SSH configuration example and
choose any local alias; `rackforge` is used in the examples:

```powershell
ssh rackforge
```

No usernames, private keys or passwords are stored in the repository.

## Automated builds

Every push to `main` runs `.github/workflows/build-main.yml` and publishes
three plugin-independent artifacts:

- `RackForge.exe` for Windows x86-64;
- `RackForge-debug.apk` for Android ARM64;
- `RackForge-RaspberryPi-arm64.tar.gz` for Raspberry Pi OS ARM64.

Plugins maintain their own repositories, versions, and pipelines. RackForge
only ships hosts capable of installing and running them.

### Windows preview

Download the Windows artifact or release, extract the complete directory and
run `RackForge.exe`. On first start, RackForge asks where to store plugins,
performances and settings. The recommended location uses the current user's
local application-data directory; portable mode keeps `RackForgeData` beside
the executable. See [RackForge Desktop](apps/rackforge-desktop/README.md) for
audio, ASIO and build details.

### Android preview

Download `RackForge-debug.apk`, allow installation from the selected source and
install the APK. The preview build is ARM64 and debug-signed. USB MIDI devices
and class-compliant USB audio interfaces can be attached through a powered hub;
the phone speaker remains available for initial testing. See
[RackForge Android](apps/rackforge-android/README.md) for audio and background
runtime details.

## Install on Raspberry Pi

The Raspberry Pi distribution requires a Raspberry Pi 4 or 5 running the
64-bit edition of Raspberry Pi OS Lite. Instrument plugins are not bundled;
`.rfplugin` packages are installed separately from RackForge.

Download `RackForge-RaspberryPi-arm64.tar.gz` from the
[GitHub Releases page](https://github.com/kalexis1994/rackforge/releases).
On the Raspberry Pi, run the following commands as the user who will run
RackForge:

```bash
mkdir -p "$HOME/rackforge/current"
tar -xzf RackForge-RaspberryPi-arm64.tar.gz \
  -C "$HOME/rackforge/current" --strip-components=1
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install.sh"
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh"
```

The installer detects the user and home directory, installs the runtime, Web
interface, platform and controller hosts, and configures the boot services. It
does not depend on a specific username. Set `RACKFORGE_USER` and
`RACKFORGE_ROOT` to use a custom identity or installation directory.

After installation, the Web interface is available on port `8787` of the
Raspberry Pi. Find its address with:

```bash
hostname -I
```

From another device on the same network, open
`http://RASPBERRY_PI_ADDRESS:8787`, install an instrument `.rfplugin`, and
select the MIDI and audio devices. Enable the reversible appliance
optimizations with:

```bash
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh" --optimize
sudo reboot
```

## Current status

- Windows x86-64, Android ARM64, and Raspberry Pi OS ARM64 hosts are built from
  the same portable contracts and published by GitHub Actions.
- PLAY and LIVE are host-owned modes shared by the Web, desktop, Android, and
  `little@1` controller surfaces.
- LIVE provides persistent Racks, Songs and Setlists. Its graph editor supports
  typed MIDI/audio nodes, child Racks, connections, drag positioning, wheel
  zoom, panning and portable labels; the viewport itself stays device-local.
- Portable `.rfplugin` installation, version activation, duplicate prevention,
  program selection, host presets and embedded PLAY/CONFIG Web surfaces are
  implemented. Local archives are supported by Desktop and Android, while the
  Web Store installs packages from configured signed repositories.
- Plugins can use a host-owned cross-platform resource explorer and install
  selected files into private plugin storage. Portable plugins can also expose
  native program editors with preview, save, replace and cancel operations.
- Windows supports WASAPI and ASIO device discovery. Android uses its native
  low-latency audio path and USB MIDI. Raspberry Pi uses ALSA and runs headless.
- MIDI hotplug recovery releases held notes and reconnects without restarting
  the application.
- The official Arturia KeyLab Essential mk3 `.rfcontroller` package provides
  the LITTLE display, navigation, dimmed LEDs and pads, master level and pan,
  long-press return, and the host escape chord across the three platforms.
- Stable PLAY/LIVE context, active plugin/program, master level and pan are
  checkpointed and restored. Windows and Raspberry Pi also prevent concurrent
  RackForge audio engines.
- The HTTP server is disabled by default on desktop. Network exposure remains
  an explicit user setting.
- The `v0.1.x` packages are preview builds: Windows is not production-signed
  and Android still uses a debug signing configuration.

The next milestone focuses on reliability gates, package conformance,
measured audio/MIDI stress tests, and smaller internal implementation modules.

## Documentation

- [Runtime layout and process model](docs/RUNTIME.md)
- [Plugin development](docs/PLUGIN_DEVELOPMENT.md)
- [Portable plugin runtime](docs/architecture/portable-plugin-runtime.md)
- [Plugin Web API](docs/WEB_PLUGIN_API.md)
- [LIVE performance and rack graphs](docs/architecture/live-performance.md)
- [Technical roadmap](ROADMAP.md)
