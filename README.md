# RackForge

<p align="center">
  <img src="assets/brand/rackforge-logo.svg" width="360" alt="RackForge logo" />
</p>

<p align="center">
  <strong>Turn a computer, phone, or Raspberry Pi into a playable musical instrument.</strong>
</p>

<p align="center">
  <a href="https://kalexis1994.github.io/rackforge/"><strong>▶ TRY RACKFORGE WEB</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/kalexis1994/rackforge/releases/latest"><strong>DOWNLOAD THE LATEST RELEASE</strong></a>
</p>

RackForge is a cross-platform instrument and live-performance host. Connect a
MIDI keyboard, choose an instrument, combine sounds into Racks, organize a show
into Songs and Setlists, and play without building a DAW session first.

RackForge uses the same musical model on Windows, Linux, Android, and
Raspberry Pi. Portable `.rfplugin` instruments bring their sound and interface
with them, while RackForge owns the audio, MIDI, routing, presets, and live
performance workflow.

> RackForge `v0.1.x` is a public preview. It is ready to explore and play, but
> native packages are not yet production-signed.

## Try it first

The fastest way to understand RackForge is to play it:

### [Open RackForge Web →](https://kalexis1994.github.io/rackforge/)

It runs locally inside the browser. There is no account, installer, or remote
audio server, and the instruments and performances you save remain in that
browser.

To hear your first sound:

1. Open RackForge Web and click or tap once to enable browser audio.
2. Open **Touch Controller** and play the on-screen keyboard or pads.
3. Open **PLAY** to choose Concert Grand or RF-106 and select a program.
4. Optionally connect a USB MIDI keyboard and allow MIDI access when asked.

Chrome and Edge provide the most complete Web MIDI support. Firefox asks for
explicit permission, while Safari currently does not expose Web MIDI. The
on-screen controller works without MIDI hardware.

You can install the Web edition from the browser menu and open it like an app.
It continues to work offline after its files have been cached. Browser audio is
excellent for exploring RackForge; use a native edition when you need the
lowest latency or a dedicated audio interface on stage.

## Find your way around RackForge

RackForge separates playing, performance design, and system configuration so
each screen has a clear job.

| Area | What it is for |
| --- | --- |
| **Home** | See what is ready and jump directly into an installed instrument. |
| **PLAY** | Choose one instrument, browse its programs, and play it immediately. |
| **LIVE** | Perform with Racks, Songs, Song Parts, and Setlists. |
| **Touch Controller** | Play a keyboard or pad grid from a mouse or touchscreen. |
| **Plugin Manager** | Install, activate, configure, or remove instruments. |
| **Audio & MIDI** | Select interfaces, decide which MIDI inputs RackForge may use, and correct each keyboard's velocity. |
| **Settings** | Configure the features available on the current device. |

The interface adapts to the screen instead of changing the workflow. On a
phone it prioritizes touch and stage controls; on a desktop it keeps navigation,
editors, plugins, and the resizable Touch Controller visible together.

## From an instrument to a complete show

RackForge uses a small set of musical building blocks:

```text
Instrument → Rack → Song Part → Song → Setlist
```

- An **Instrument** is a plugin you can play, such as Concert Grand or RF-106.
- A **Rack** layers and routes one or more instruments. It can include MIDI
  channel maps, key ranges, transposition, and velocity shaping.
- A **Song Part** is the playable graph for one section of a song. It may use
  instruments directly, complete Racks, or both at the same time.
- A **Song** orders its parts so you can move from intro to verse, chorus, solo,
  or any structure you choose.
- A **Setlist** orders Songs for a rehearsal or performance.

Use **PLAY** when you want one sound quickly. Use **LIVE** when the arrangement
itself matters. RackForge keeps instrument state separate from the performance
graph, so changing stage routing does not rewrite the plugin.

## Standard and Minimal editions

Native CI builds are published in two editions for every supported platform:

- **Standard** includes Concert Grand and the officially pinned instruments —
  today RF-106 and RF-5 — so a new installation can be played immediately.
- **Minimal** includes no instrument plugins. It keeps the complete RackForge
  host, Plugin Manager, controller support, and performance tools so you can
  install only the instruments you want.

Both editions use the same runtime and can install the same `.rfplugin`
packages. The edition only controls what is bundled at build time; it does not
limit features or plugin compatibility.

## Instruments and portable plugins

Standard installations include:

- **RF - Concert Grand**, a physically modelled piano developed in this
  repository — see below.
- **RF-106**, a portable virtual-analog synthesizer with its own PLAY interface
  and RackForge parameter mapping.
- **RF-5**, a five-voice programmable polyphonic synthesizer.

Other open-source instruments include:

- [RF-106 source and releases](https://github.com/kalexis1994/rackforge-plugin-rf-106)
- [RF-5 source and releases](https://github.com/kalexis1994/rackforge-plugin-rf-5)
- [RF-Soundfonts](https://github.com/kalexis1994/rackforge-plugin-rf-soundfonts),
  a SoundFont instrument that includes the sampled YDP Grand Piano

### RF - Concert Grand

The piano that ships with RackForge is not a sample library. There are no
recordings in the package: every note is computed as it is played, from a
string that is stiff and lossy, a hammer with felt that hardens under the blow,
a soundboard that radiates, and a room with a lid and a pair of microphones in
it. The whole model is written down, measured against real instruments and the
published literature, in [docs/PIANO_MODEL.md](docs/PIANO_MODEL.md).

What that buys you is an instrument that changes as a piano changes rather than
as a preset changes:

- **Sixteen instruments, not sixteen tone settings.** Concert Grand, Mellow,
  Bright and Intimate, then the scales of real pianos — Concert 308 down through
  Salon 211, Parlour 185, Baby 150, two uprights, a player upright and a
  fortepiano. The size is a dimension of the model, so a shorter piano is
  shorter everywhere: tension, inharmonicity, decay and the bass break all move
  with it.
- **A room you can move around in.** Lid position, microphone distance and the
  pair's spacing are geometry, not reverb: each capsule hears the soundboard and
  its image in a hinged, finite lid.
- **The action, audibly.** Hammer speed spans the published range, the felt is
  measured per note, the dampers land, and the una corda pedal is read.
- **162 parameters, all of them the model's own** — the sound, the room and its
  microphones, the action and its noises — plus a Model page that exposes the
  physical constants themselves and a button that puts them back.

It reads 16-bit velocity where the host can supply it, and the per-keyboard
velocity reading below applies before it ever sees a note.

A portable `.rfplugin` can carry its engine, Web interface, programs, artwork,
and resources in one package. Install it from **Plugin Manager** and RackForge
will validate it before activation. Compatible portable packages can be moved
between the native platforms without creating a platform-specific preset.

## Choose where to play

| Platform | Best for | Audio and MIDI |
| --- | --- | --- |
| **Web** | Trying RackForge instantly or controlling a remote setup | Browser audio, Touch Controller, and Web MIDI where supported |
| **Windows x86-64** | Creating instruments, Racks, Songs, and Setlists | WASAPI, ASIO, and Windows MIDI |
| **Android ARM64** | A compact touchscreen instrument | Low-latency native audio and USB MIDI/audio |
| **Raspberry Pi OS ARM64** | A dedicated instrument that starts at boot | ALSA, USB MIDI, Web control, and system services |
| **Linux x86-64** | A PC-based headless or Web-controlled instrument | ALSA, Linux MIDI, Web control, and system services |
| **Windows VST3 x86-64** | Opening RackForge as an instrument inside a DAW | DAW-provided MIDI, timing, state, and stereo audio |

Native packages are available from the
[latest RackForge release](https://github.com/kalexis1994/rackforge/releases/latest).

## Install a native edition

### Windows

1. Download
   [`RackForge-Windows-x86_64.exe`](https://github.com/kalexis1994/rackforge/releases/latest/download/RackForge-Windows-x86_64.exe).
2. Run it and choose where RackForge should store its plugins, performances,
   and settings.
3. Open **Audio & MIDI**, enable the controller you want to use, and select an
   audio output.
4. Open **PLAY** and start with Concert Grand or RF-106.

Windows builds are currently unsigned, so Windows may ask you to confirm that
you trust the application. The [Desktop guide](apps/rackforge-desktop/README.md)
covers ASIO, portable mode, and troubleshooting.

### Android

1. Download
   [`RackForge-Android-arm64.apk`](https://github.com/kalexis1994/rackforge/releases/latest/download/RackForge-Android-arm64.apk).
2. Allow installation from your browser or file manager and install the APK.
3. Connect a MIDI controller and, optionally, a class-compliant USB audio
   interface through a powered hub.
4. Select the devices in **Audio & MIDI** and play.

The preview APK is ARM64 and debug-signed. The phone speaker is enough for a
first test; an external interface generally provides the best stage latency.
See the [Android guide](apps/rackforge-android/README.md).

### Raspberry Pi in one command

Use a Raspberry Pi 4 or 5 with the 64-bit edition of Raspberry Pi OS Lite. Run
this as the regular user that will run RackForge, not with `sudo`:

```bash
bash -o pipefail -c 'curl -fsSL https://raw.githubusercontent.com/kalexis1994/rackforge/main/platforms/raspberry-pi/install-release.sh | bash'
```

The installer verifies the architecture and release checksum, preserves the
previous installation for rollback, installs the Web interface and boot
services, and enables the bundled instruments. When it finishes, open:

```text
http://RASPBERRY_PI_ADDRESS:8787
```

from a phone, tablet, or computer on the same network. The
[Raspberry Pi guide](platforms/raspberry-pi/README.md) covers audio setup,
service management, and diagnostics.

<details>
<summary><strong>Review, pin, or customize the Raspberry Pi installation</strong></summary>

Download and inspect the installer before running it:

```bash
curl -fL https://raw.githubusercontent.com/kalexis1994/rackforge/main/platforms/raspberry-pi/install-release.sh -o install-rackforge.sh
less install-rackforge.sh
bash install-rackforge.sh
```

Install a specific release or enable the optional reversible appliance
optimizations:

```bash
RACKFORGE_VERSION=v0.1.7 bash install-rackforge.sh
RACKFORGE_OPTIMIZE=1 bash install-rackforge.sh
```

The installer detects the current user and never assumes a fixed username.
Advanced installations may set `RACKFORGE_ROOT`.

</details>

### Linux x86-64

1. Download `RackForge-Linux-x86_64.tar.gz` from the latest release.
2. Extract it and enter the `rackforge` directory.
3. Run `bash platforms/linux-x86_64/install.sh` as the ordinary user who will
   run RackForge.
4. Open `http://localhost:8787` and configure **Audio & MIDI**.

This edition uses the same headless Core and adaptive Web interface as
Raspberry Pi, built natively for 64-bit Intel and AMD computers. See the
[Linux x86-64 guide](platforms/linux-x86_64/README.md).

### Windows VST3 preview

The release includes an installable `RackForge.vst3` bundle. Copy that complete
directory to a VST3 location scanned by the DAW; the loose
`rackforge-vst3.dll` is a diagnostic module, not the normal installation.

The VST edition receives MIDI, timing, and audio from the DAW and stores its
state in the DAW project. It never opens a second audio device. A physical
`.rfcontroller` can be used, but only one focused RackForge instance can own a
unique controller at a time. See the
[VST3 host architecture](docs/architecture/vst3-host.md).

## Hardware controllers and LITTLE

RackForge can do more than receive notes. Portable `.rfcontroller` packages
translate a controller's keys, faders, encoders, pads, lights, and display into
RackForge actions without placing controller-specific rules inside an
instrument.

The Arturia KeyLab Essential mk3 is the first reference controller. Its package
provides:

- the LITTLE display and navigation;
- `SETTINGS > MIDI`, to choose which keyboards are listened to and to correct
  each one's velocity from the hardware itself;
- `SETTINGS > WEB INTERFACE > ACCESS PIN`, so a machine with no PIN can be
  claimed at the instrument rather than only from a browser;
- dimmed button and pad lighting;
- standard instrument parameter controls;
- master volume and pan;
- long-press return to the active instrument;
- a hardware escape chord that stops sound and returns to the main menu.

The same controller package is used on Windows, Linux x86-64, Android, and
Raspberry Pi.

## Save it, move it, play it

RackForge restores the active mode, instrument, program, master controls, and
performance library between sessions. Portable `.rfpreset` files let you
export an instrument state with plugin identity and version information, then
import it on another compatible RackForge host.

RackForge is being built toward a simple outcome: prepare a performance on a
desktop, move it to a phone or Raspberry Pi, reconnect the controller and audio
interface, and keep playing the same show.

## Public preview status

Already available:

- cross-platform PLAY and LIVE state;
- portable plugin installation, activation, configuration, and removal;
- Racks, Songs, Song Parts, Setlists, and visual routing graphs;
- embedded PLAY and CONFIG interfaces;
- portable presets and plugin-private resource storage;
- MIDI hotplug recovery and session restoration;
- a velocity reading per keyboard, drawn as a curve, auditioned while it is
  drawn and applied where MIDI arrives;
- an engine that starts with no audio device and no MIDI attached, and adopts
  an interface when one appears;
- Touch Controller keyboard and pads;
- controller displays, LEDs, encoders, and standard parameter mappings;
- protection against concurrent native audio engines.

Current limitations:

- Windows packages are not code-signed.
- Android packages are ARM64 and debug-signed.
- Linux x86-64 currently uses a headless/Web interface rather than a native
  desktop window.
- Raspberry Pi requires a 64-bit Raspberry Pi OS userspace.
- Optional instruments such as RF-Soundfonts are separate downloads.
- The optional Desktop HTTP server is disabled by default.
- Browser latency and MIDI support depend on the browser and device.

## Guides and help

- [Plugin ABI](docs/PLUGIN_ABI.md) — the `wasm-v1` contract, for any language
- [Windows Desktop guide](apps/rackforge-desktop/README.md)
- [Android guide](apps/rackforge-android/README.md)
- [Raspberry Pi guide](platforms/raspberry-pi/README.md)
- [Linux x86-64 guide](platforms/linux-x86_64/README.md)
- [Portable preset format](docs/RFPRESET.md)
- [LIVE performance model](docs/architecture/live-performance.md)
- [Experience system and performance budgets](docs/architecture/experience-system.md)
- [Reliability qualification](docs/RELIABILITY.md)
- [Runtime and process layout](docs/RUNTIME.md)
- [Technical roadmap](ROADMAP.md)

## Build instruments for RackForge

RackForge plugins use portable host contracts instead of platform-specific
audio or filesystem APIs. Start with:

- [Plugin development](docs/PLUGIN_DEVELOPMENT.md)
- [Portable plugin runtime](docs/architecture/portable-plugin-runtime.md)
- [Plugin Web API](docs/WEB_PLUGIN_API.md)

Instrument repositories own their release pipelines and assets such as sample
libraries or firmware. RackForge validates their packages against the public
host contracts before activation.

<details>
<summary><strong>Contributing and repository architecture</strong></summary>

Every Pull Request targeting `main` must pass contracts, Web validation,
Windows, VST3, Linux x86-64, Android, Raspberry Pi, and browser-demo checks.
Merging to `main` rebuilds the release artifacts and deploys the Web edition.

```text
MIDI controller
  • keys, pads, encoders, buttons
           │ native MIDI
           ▼
Platform MIDI + .rfcontroller package
  • discovers and reconnects hardware
  • maps controls to host intents
  • renders LITTLE, LEDs, and pads
           │
           ▼
RackForge Core
  • authoritative musical state
  • portable plugins and programs
  • Racks, Songs, and Setlists
  • routing, mixing, and audio
           │
           ▼
Platform audio
  • ALSA / WASAPI / ASIO / native Android audio
           │
           ▼
Built-in audio / USB interface
```

| Area | Responsibility |
| --- | --- |
| `crates/` | Core, APIs, SDK, and portable plugin runtime |
| `apps/` | Windows Desktop, Android, VST3, and headless/Web hosts |
| `platforms/` | Platform adapters, installation, and deployment |
| `hardware/` | Drivers and packages for supported MIDI controllers |
| `plugins/` | Bundled instruments and conformance fixtures |
| `web/` | RackForge's adaptive interface |

The host owns authoritative engine, bank, and performance state. Controllers
send physical events and render the state they receive. A handshake rebuilds
the control surface after either side restarts.

</details>

## Licence

RackForge itself — the host, its interfaces and the instruments in this
repository — is [GPL-2.0-or-later](LICENSE).

**`rackforge-plugin-sdk` is not.** It is dual
[MIT](crates/rackforge-plugin-sdk/LICENSE-MIT) or
[Apache-2.0](crates/rackforge-plugin-sdk/LICENSE-APACHE), at your choice,
because a guest SDK that a third party links into their own instrument must
not decide that instrument's licence. Plugins you build for RackForge are
yours, under whatever terms you choose.

The same is true by construction of the ABI in
[docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md): a plugin that implements it directly,
as [`plugins/gain-c/gain.c`](plugins/gain-c/gain.c) does, links nothing of ours
at all.
