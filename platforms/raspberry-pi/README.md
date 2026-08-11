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

The default installation root is `$HOME/rackforge/current`; the installer
resolves the actual user and home directory and never depends on a hardcoded
username.

## Build

From Windows or another development machine, use the repository CI or the
platform scripts. The release pipeline publishes
`RackForge-RaspberryPi-arm64.tar.gz`.

## Install

```bash
mkdir -p "$HOME/rackforge/current"
tar -xzf RackForge-RaspberryPi-arm64.tar.gz \
  -C "$HOME/rackforge/current" --strip-components=1
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install.sh"
bash "$HOME/rackforge/current/platforms/raspberry-pi/scripts/install-appliance.sh"
```

The second command installs the supervised appliance services. Optional
real-time and power optimizations are applied with `--optimize` and can be
rolled back locally.

Plugins are installed separately as `.rfplugin` packages. Proprietary banks
and ROMs are never bundled with RackForge.
