# RackForge Release Contents

Every native release includes Concert Grand and every officially pinned plugin.
The host, shared Web interface, Plugin Manager, platform audio and MIDI support,
performance documents, and bundled controller integration are included too.
Players can install other portable plugins later.

## Build contract

The release workflow builds the Standard edition on every platform. The build
entry points retain their edition option for development, but the release
assembler accepts only the six Standard artifacts from one CI run: the
Windows executable and VST3 bundle, the Linux x86-64 archive and Flatpak, the
Raspberry Pi archive and the Android APK.

Every release bundle contains `build-info.txt`. CI inspects the Android, Linux,
and Raspberry Pi archives and verifies their bundled plugins.

What a Standard artifact is expected to carry is read from
`tools/fetch-official-plugins.py`, not written out beside the check. Naming
the packages twice is how a check goes stale: the first version of this one
named RF-106 alone, and by the time it ran the builds also carried RF-5, so it
would have rejected a correct artifact. Adding an official instrument now
means editing the pins and nothing else.

Controller packages are host integration rather than instruments and remain in
every release.

## Concert Grand ownership

Concert Grand is developed in
[rackforge-plugin-concert-grand](https://github.com/kalexis1994/rackforge-plugin-concert-grand).
Its DSP, interface, tests and laboratory reports live there. RackForge consumes
its versioned, checksummed release through `tools/fetch-official-plugins.py`,
as it does the other official instruments. The pin marked `default` is stored
beside the other official packages so native hosts embed the startup instrument
once. The browser receives that same package under its declared extraction name.
