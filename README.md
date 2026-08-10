# RackForge

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
| `apps/`      | Desktop and headless/Web executables.                                     |
| `platforms/` | Platform-specific integrations, starting with Raspberry Pi.               |
| `hardware/`  | Drivers and packages for supported MIDI controllers.                      |
| `plugins/`   | Minimal conformance fixtures; instruments live in their own repositories. |
| `web/`       | RackForge's adaptive SPA.                                                 |
| `firmware/`  | Device research and firmware.                                             |

## Signal flow

```text
Keys, pads, and controls
           │
           ▼
KeyLab firmware
  • detects RackForge
  • sends user intents
  • displays menus and status
           │ USB
           ▼
RackForge Core
  • authoritative musical state
  • plugins and engines
  • banks and performances
  • mixing and audio
           │
           ▼
      USB DAC / Scarlett
```

The host owns the authoritative engine, bank, and performance state. The
controller sends physical events, and its driver renders the state it receives.
If either side restarts, a handshake rebuilds the control surface without
depending on implicit state.

## Remote development

The Raspberry Pi is accessed through a dedicated key and a local SSH alias:

```powershell
ssh rackforge
```

No passwords are stored in the repository. Reproducible connection,
synchronization, and diagnostic tools live in `platforms/raspberry-pi/dev/`.

## Automated builds

Every push to `main` runs `.github/workflows/build-main.yml` and publishes
three plugin-independent artifacts:

- `RackForge.exe` for Windows x86-64;
- `RackForge-debug.apk` for Android ARM64;
- `RackForge-RaspberryPi-arm64.tar.gz` for Raspberry Pi OS ARM64.

Plugins maintain their own repositories, versions, and pipelines. RackForge
only ships hosts capable of installing and running them.

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

- Raspberry Pi OS Lite / Debian 13 arm64, without a graphical environment.
- Rust, C/C++, CMake, Ninja, ALSA, and udev toolchains installed.
- SysEx display communication verified with the current Arturia firmware.
- KeyLab note-on/note-off input verified directly on Raspberry Pi.
- Nuked-SC55 builds natively for ARM64; users must provide their own ROMs.
- Sound Canvas VA 1.1.2 ABI validated and internal banks cataloged with Rust
  tools; these are not directly compatible Nuked-SC55 ROMs.
- Wave ROM reader and FCE-DPCM decoder validated natively on ARM64 with output
  matching Windows.
- Native SCVA 1.1.2 tone, map, and sample descriptor resolver; `Piano 1/C4`
  already produces a reproducible preview through the Scarlett.
- Safe N32G455 bare-metal scaffold.
- DOOM port retained as a test bench and potential future feature.

The immediate priority is to complete the first
KeyLab → Nuked-SC55 → Scarlett path and integrate it into the headless daemon.
