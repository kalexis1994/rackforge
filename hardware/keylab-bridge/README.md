# RackForge Arturia controller driver

Packaged controller driver for the Arturia KeyLab Essential 61 mk3. The binary
belongs to `org.rackforge.arturia-keylab-essential-mk3.rfcontroller`; the
generic controller host contains no Arturia identity or protocol knowledge.

The certified profile implements `little@1`. Endpoint identity, driver
identity, and layout support are validated before SysEx is sent, including when
a user supplies a port selector manually. Unknown endpoints never receive
display commands.

The driver uses only the official DAW MIDI/SysEx protocol. It does not update
firmware or write templates and user memories. It enters DAW Program only for
the acquired session and restores the previous Arturia surface on exit.

Protocol evidence and exact messages live in [`PROTOCOL.md`](PROTOCOL.md).

## Development commands

Windows examples use the MSVC toolchain:

```powershell
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\hardware\keylab-bridge\Cargo.toml -- list
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\hardware\keylab-bridge\Cargo.toml -- demo
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\hardware\keylab-bridge\Cargo.toml -- demo --execute --seconds 30
cargo +stable-x86_64-pc-windows-msvc run --manifest-path .\hardware\keylab-bridge\Cargo.toml -- menu-demo --execute --seconds 30
```

`list` and dry-run modes do not acquire the surface. Commands that transmit
messages require the explicit `--execute` flag and always restore the keyboard
when the session ends.

The driver also exposes an isolated process protocol used by the controller
host. Startup validates API version, package identity, target, and declared
capabilities before opening MIDI.

## LITTLE menu model

The menu is host-owned and rendered by `rackforge-surface-runtime`. This
driver only:

1. translates physical messages into logical LITTLE inputs;
2. forwards typed host controls and actions;
3. encodes the resulting logical screen into Arturia SysEx;
4. maintains heartbeat, LED state, and restoration.

The root contains:

```text
LIVE
PLAY
CONFIG
```

- **PLAY** selects one active plugin. Inside it, RackForge presents portable
  PRESETS first, native plugin PROGRAMS second, then any declarative editor
  sections published by the plugin.
- **LIVE** selects performances such as racks, songs, and setlists.
- **CONFIG** exposes host-owned plugin, audio, network, and system settings.

Plugin loading displays a bounded spinner and blocks stale input until Core
publishes the selected instance snapshot. Long BACK returns to the active mode
and restores the active plugin/sound focus. Long OK + BACK performs the
host-owned emergency return and global stop.

Program editing uses a host-owned draft. Plugins expose a declarative editor
tree and opaque field IDs; the controller never assumes plugin JSON paths.
Audition leases are renewed while editing and restored on cancel, timeout, or
disconnect.

## Semantic RackForge parameters

- Fader 9: `rackforge.master.level`.
- Encoder 9: `rackforge.master.pan`, interpreted relatively to prevent reconnect jumps.

## Reserved action

- PART: open keyboard parts, set a split with a held note, or clear the split
  after a long hold.

Reserved messages are registered with Core before MIDI input opens and are
removed from the plugin MIDI stream.

## Display and LEDs

The screen uses the native header, two-line body, and four-button footer.
Transient global-parameter feedback replaces only the header and restores it
after inactivity. Formatting is owned by RackForge, not by the Arturia driver.

The shared controller runtime applies a dim blue profile to mode buttons,
transport, contextual buttons, and pads. Context buttons may temporarily
brighten for focus or press feedback. The full RGB range is switched off before
DAW disconnect.

## Persistent service

On Raspberry Pi, `serve --execute` supervises both the MIDI endpoint and the
Core control socket. It waits for late USB enumeration, reacquires after
disconnect, refreshes the menu from authoritative session state, and releases
held notes through Core's MIDI supervision.

Build and install the packaged service with:

```bash
cargo build --release --bin rackforge-arturia-keylab-essential-mk3-driver
cargo build --release --bin rackforge-controller-host
cd hardware/controllers/arturia-keylab-essential-mk3
bash ./install.sh
systemctl status rackforge-controller-host.service
```

Desktop and Android use the same controller state machine and protocol
serialization with their platform-specific MIDI transports.
