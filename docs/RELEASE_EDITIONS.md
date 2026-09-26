# RackForge Release Contents

Every native release includes Concert Grand and every officially pinned plugin.
The host, shared Web interface, Plugin Manager, platform audio and MIDI support,
performance documents, and bundled controller integration are included too.
Players can install other portable plugins later.

## Build contract

The release workflow builds the Standard edition on every platform. The build
entry points retain their edition option for development, but the release
assembler accepts only the five Standard artifacts from one CI run.

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

Source ownership and release bundling are independent. A future migration to a
dedicated `rackforge-plugin-concert-grand` repository would make its release
lifecycle match the other official plugins, but should be handled separately.
After that migration, builds can consume a pinned, checksummed release the same
way they consume the other official instruments, and the expected set would
follow from the pins with no further edit.
