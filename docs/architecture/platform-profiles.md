# Platform and hardware profiles

Status: detection, privileged host and LITTLE Wi-Fi surface implemented;
schema `1`

## Three independent axes

RackForge does not use one string to mean binary compatibility, physical
hardware and available system features.

1. **Binary platform** selects executable code, for example
   `linux-aarch64`, `linux-x86-64` or `windows-x86-64`.
2. **Hardware profile** identifies a tested appliance, for example
   `org.rackforge.hardware.raspberry-pi-4`.
3. **Capabilities** describe optional host features and the allowlisted
   provider implementing each one.

A Raspberry Pi 4 and Pi 5 may both execute a `linux-aarch64` plugin. A generic
Linux computer can expose Wi-Fi management through NetworkManager without
pretending to be a Raspberry Pi.

Plugins consume the normal RackForge APIs. They do not detect boards, invoke
system tools or require a hardware profile unless they explicitly offer an
optional optimized binary.

## Detection and fallback

RackForge always has a binary platform. Hardware detection may resolve a known
profile or remain generic. Unknown hardware is valid and receives only
capabilities proven by an available provider.

Known Raspberry Pi models are detected from device tree model information. A
revision such as `Rev 1.4` is metadata, not part of profile identity.

UI entries are capability-driven:

- `system.network.wifi.manage.v1` shows Wi-Fi configuration;
- `system.boot.fast-audio.v1` allows appliance boot management;
- `system.telemetry.v1` allows temperature, throttling and power status.

When a capability is absent, the entry is omitted. It is not shown disabled.

## Provider boundary

Profiles select only providers compiled into and accepted by RackForge.
Descriptors cannot contain shell commands.

The initial providers are:

- `org.rackforge.provider.network-manager`;
- `org.rackforge.provider.systemd`;
- `org.rackforge.provider.raspberry-pi`.

Privileged operations live in a small platform host. It exposes a bounded,
versioned Unix-socket protocol and an explicit command allowlist. Controller
drivers, plugins, the web server and the audio process remain unprivileged.

Wi-Fi credentials are write-only:

- they are accepted only by the platform host;
- they are handed to NetworkManager;
- they are never returned through the API, display or logs;
- controller surfaces may reconnect saved networks without seeing secrets.

The LITTLE surface groups profiles under `KNOWN` and scan results without an
existing profile under `DISCOVERED`. Known networks expose connect/disconnect
and forget actions. Selecting a secured discovered network opens the standard
four-button secret editor before connection.

Platform operations run outside the controller's display loop. The reusable
`rackforge-ui::Spinner` component renders the portable ASCII sequence
`| / - \\` while work is pending, so OLED refresh, hardware input and health
checks remain responsive. The component is not Wi-Fi-specific and can be used
by bank, plugin and hardware-loading flows.

Passphrases are redacted from debug output, cleared when the editor or request
is dropped, and sent only over the protected local platform socket. The host
uses NetworkManager's `passwd-file` mechanism with an ephemeral `0600` file in
`/run`; it is removed immediately after activation. Secrets are never placed in
process arguments or RackForge logs. NetworkManager owns the resulting
persistent profile.

## Appliance boot

Audio readiness must not depend on Internet or `network-online.target`.

For Raspberry Pi appliance profiles, service readiness follows the shared
[startup availability policy](startup-availability.md). Audio is the only
critical boot path; controller and management layers follow it:

```text
local filesystems
  +-> audio Core -> first successful device period
       +-> platform host -> controller host
            +-> NetworkManager -> Web host
```

Core restores the last stable PLAY/LIVE selection and retries bounded hardware
discovery for MIDI and audio devices. systemd does not consider Core ready when
the process merely starts: Core publishes readiness after its first successful
audio period. CPU and I/O startup weights reinforce this order without fixed
delays.

The metric is power-on to `READY_TO_PLAY`, not the time at which every general
OS background unit is idle. NetworkManager wait-online, package updates and
other appliance-image policies are optimized separately and must never be
required by the audio path.

### Raspberry Pi OS Lite appliance profile

The recommended base image is a clean Raspberry Pi OS Lite installation. The
official Raspberry Pi documentation recommends Lite for headless systems and
uses NetworkManager by default on current releases. RackForge preserves
NetworkManager because both the Web UI and hardware Wi-Fi surface depend on it.

`scripts/optimize-appliance.sh` owns the post-provisioning transition:

```bash
bash scripts/optimize-appliance.sh audit
bash scripts/optimize-appliance.sh apply
bash scripts/optimize-appliance.sh rollback
```

`apply` refuses to proceed until a local user, SSH host keys, a persistent
network profile and a completed cloud-init first boot exist. It then:

- backs up every changed unit and service state under
  `/var/lib/rackforge/appliance/rollback`;
- disables cloud-init for subsequent fixed-appliance boots;
- disables NetworkManager wait-online because audio, display and Web startup
  do not require network readiness;
- enforces mode `0600` on Netplan policy;
- removes network ordering from the platform and Web processes;
- disables camera and DSI display auto-detection on the headless appliance,
  while preserving HDMI/KMS as a local recovery path.

The original Raspberry Pi boot configuration is included in the rollback
snapshot. Re-running `apply` upgrades an already-applied profile without
discarding its original backups.

Bluetooth, Avahi, EEPROM maintenance, filesystem checks, NetworkManager and SSH
remain enabled by default. They are useful capabilities or maintenance paths,
and removing them offers little improvement on the measured critical path.

## Current Raspberry Pi 4 baseline

Measured on the development Pi 4 before the appliance profile:

- kernel plus userspace reports about 49 seconds to full boot;
- `multi-user.target` is reached at about 11.7 seconds;
- controller host starts at about 9.0 seconds and detects its package at 9.3;
- web host becomes ready at about 15.1 seconds;
- NetworkManager wait-online consumes about 33.8 seconds;
- audio Core originally had no boot service and could not become ready
  automatically.

The supervised `rackforge-audio.service` is now installed and does not depend
on network readiness. It runs `rackforge-core resume` with a versioned startup
document, restores the stable session checkpoint and retries after missing
hardware or process failure.

`rackforge-platform-host.service` is also installed. Its socket is owned by
`root:rackforge` with mode `0660`; controller and web processes remain
unprivileged members of that group. The LITTLE Wi-Fi surface supports:

- current SSID and signal;
- known-profile activation, disconnection and forgetting;
- discovery and connection to new open or secured networks;
- masked password entry for secured networks;
- Wi-Fi radio on/off;
- explicit success/failure feedback.

The first supervised reboot measured `READY_TO_PLAY` at 9.61 seconds, Web at
15.34 seconds and OLED acquisition at 15.45 seconds from kernel time zero.

After applying the appliance profile, `READY_TO_PLAY` measured 6.61 seconds,
Web 6.22 seconds and OLED acquisition 9.76 seconds. The Pi firmware handed
control to the ARM kernel at 9.85 seconds after power-on. The resulting expected
physical readiness is approximately 16.5 seconds for audio and 19.6 seconds for
HOME on the controller.

The KeyLab's five-second USB boot guard is measured from the device's udev
initialization timestamp, not from controller-host process startup. This keeps
the proven-safe delay before Arturia SysEx while avoiding duplicate waiting.
OLED acquisition still requires an explicit acknowledgement and retains its
retry/health protocol.

The remaining pre-kernel path is dominated by the Pi 4 EEPROM/firmware and the
16 MB host-dependent initramfs. The EEPROM correctly tries SD before USB and
the board reports no undervoltage or throttling. The initramfs policy is already
`MODULES=dep`; it is not removed because preserving a bootable recovery path is
more important than an unverified sub-second gain.
