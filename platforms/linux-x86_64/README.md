# RackForge for Linux x86-64

This distribution turns a 64-bit Intel or AMD Linux machine into a RackForge
host controlled through the same Web interface used by Raspberry Pi. It uses
ALSA for low-latency audio, Linux MIDI discovery, the portable `.rfplugin`
runtime, and the `.rfcontroller` host.

## Requirements

- a systemd-based x86-64 Linux distribution;
- ALSA and udev runtime libraries;
- an ordinary user account with `sudo` access.

Download and extract the Linux x86-64 release archive. Enter the extracted
`rackforge` directory and run the installer as the user who will run RackForge.
[`RackForge-Linux-x86_64.tar.gz`](https://github.com/kalexis1994/rackforge/releases/latest/download/RackForge-Linux-x86_64.tar.gz)

```bash
bash platforms/linux-x86_64/install.sh
```

The default root is `$HOME/rackforge`. Override it with an absolute
`RACKFORGE_ROOT` path if necessary. The installer enables the Web, controller,
platform and supervised audio services. Open `http://localhost:8787` and activate
an instrument to start audio. The output profile is stored in
`$RACKFORGE_ROOT/config/audio.toml`.
[Platform profiles and startup](../../docs/RUNTIME.md#platform-profiles-and-startup)

The packaged Web interface may also be opened from another device on the LAN.
Treat that network as trusted until authentication has been configured.
