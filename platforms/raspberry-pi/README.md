# Raspberry Pi platform

This directory contains only the Raspberry Pi OS Lite integration. Core, shared
APIs, portable runtime, and product plugins live outside the platform adapter.

Responsibilities:

- detect MIDI controllers and audio outputs;
- supervise controller/runtime services and reconnect after hotplug;
- configure low-latency ALSA output;
- persist performances and restore the latest session;
- expose the headless WEB surface;
- apply optional, reversible appliance optimizations.

## Target

- Raspberry Pi 4 or 5.
- 64-bit Raspberry Pi OS Lite.
- AArch64 userspace.
- No desktop environment required.

The default deployment root is `$HOME/rackforge`, and the active release lives
in its `current` directory. The installer resolves the actual user and home
directory and never depends on a hardcoded username.

## Build

From Windows or another development machine, use the repository CI or the
platform scripts. The release pipeline publishes
`RackForge-RaspberryPi-arm64.tar.gz`.

## Install

The public release can be installed or updated in one command. Run it as the
regular user that will run RackForge, not with `sudo`:

```bash
bash -o pipefail -c 'curl -fsSL https://raw.githubusercontent.com/kalexis1994/rackforge/main/platforms/raspberry-pi/install-release.sh | bash'
```

The release installer checks the ARM64 architecture, verifies the archive
against the published SHA-256 digest, and restores the previous runtime if an
update fails. It installs RF-Soundfonts with the YDP Grand Piano when the
plugin store is empty, then installs and enables the systemd services.

For a manual installation, download and verify
`RackForge-RaspberryPi-arm64.tar.gz` from the GitHub release, then run:

```bash
mkdir -p "$HOME/rackforge/current"
tar -xzf RackForge-RaspberryPi-arm64.tar.gz \
  -C "$HOME/rackforge/current" --strip-components=1
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install.sh"
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh"
```

`install-appliance.sh` installs the supervised appliance services. Optional
real-time and power optimizations are applied with `--optimize` and can be
rolled back locally.

Both supervised `resume` startup and manual `live` startup acquire the same
non-blocking audio-engine lock below `$RACKFORGE_ROOT/state`. A second engine
prints a clear error and exits before opening MIDI, plugins, or ALSA. The
kernel releases this lock automatically when the owning process exits or
crashes; the persistent lock file is not a stale PID-file gate.

RF-Soundfonts and its openly licensed YDP Grand Piano are included as the
default instrument. Other plugins are installed separately as `.rfplugin`
packages. Proprietary banks and ROMs are never bundled with RackForge.
