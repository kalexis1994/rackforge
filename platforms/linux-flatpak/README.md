# RackForge Desktop for Linux (Flatpak)

The same desktop application as on Windows -- its window, the embedded
RackForge interface, plugins, MIDI and controllers -- packaged as a Flatpak,
which installs the same way on Debian, Ubuntu, Fedora, Arch, SteamOS and any
other distribution with Flatpak. It is the Linux build for a computer that is
also used for other things; `platforms/linux-x86_64` remains the build for a
machine given over to RackForge (systemd services, ALSA held exclusively).

## Installing

Distributions without Flatpak install it once (Debian, Ubuntu:
`sudo apt install flatpak`) and add Flathub, which provides the GNOME
runtime the app runs on:

```bash
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

Then the bundle installs for the current user:

```bash
flatpak install --user RackForge-Linux-x86_64.flatpak
```

RackForge appears among the applications; `flatpak run
io.github.kalexis1994.RackForge` starts it too. Its data lives in
`~/.var/app/io.github.kalexis1994.RackForge/data/RackForge`.

## How it differs from the Windows build

- **Sound** goes through the desktop's sound server (PipeWire or PulseAudio),
  through ALSA's default device: RackForge shares the audio interface with
  everything else instead of taking it. It opens at 48 kHz where the device
  takes it.
- **The window is X11's**, through XWayland on a Wayland desktop: the embedded
  WebKitGTK view can only be placed in an X11 window.
- **File dialogs** go through the desktop portal; message boxes are GTK's.
- **Data** follows the XDG directories, with no first-start choice of folder.

## Building

`tools/build-linux-flatpak.sh` builds the Web interface and fetches the
officially pinned plugins, then has `flatpak-builder` compile the app in the
GNOME 49 SDK (`io.github.kalexis1994.RackForge.yml`) and writes
`dist/linux-flatpak/RackForge-Linux-x86_64.flatpak`. It needs `flatpak` with
`org.flatpak.Builder`, `cargo`, `pnpm`, `python3` and `git`.

Cargo fetches crates during the build (`--share=network`). A Flathub
submission needs them vendored, which is still to do.
