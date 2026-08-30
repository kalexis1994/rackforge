# RackForge Release Editions

RackForge produces two native release editions from the same source revision.
They differ only in the instrument packages included in the artifact.

| Edition | Bundled instruments | Intended use |
| --- | --- | --- |
| Standard | Concert Grand and every officially pinned instrument | Ready to play immediately |
| Minimal | None | Small, controlled installations that add plugins explicitly |

Both editions include the complete host, shared Web interface, Plugin Manager,
platform audio and MIDI support, performance documents, and bundled controller
integration. Minimal is not a reduced-function host and does not restrict which
portable plugins can be installed later.

Edition selection affects only newly bundled packages. Installing Minimal over
an existing RackForge data directory never deletes plugins the user already
installed; they remain under the user's control in Plugin Manager.

## Build contract

The PowerShell build entry points accept `-Edition Standard` or
`-Edition Minimal`. The Bash entry points read `RACKFORGE_EDITION=standard` or
`RACKFORGE_EDITION=minimal`. Standard is the default for local builds so
existing developer commands keep their current behavior.

Every release bundle contains `build-info.txt` with its edition. CI also
inspects the Android, Linux, and Raspberry Pi archives and fails if a Minimal
artifact contains any `.rfplugin` file.

What a Standard artifact is expected to carry is read from
`tools/fetch-official-plugins.py`, not written out beside the check. Naming
the packages twice is how a check goes stale: the first version of this one
named RF-106 alone, and by the time it ran the builds also carried RF-5, so it
would have rejected a correct artifact. Adding an official instrument now
means editing the pins and nothing else.

Controller packages are host integration rather than instruments and remain in
both editions.

## Concert Grand ownership

The edition split does not require moving Concert Grand out of this repository:
source ownership and release bundling are independent. A future migration to a
dedicated `rackforge-plugin-concert-grand` repository would make its release
lifecycle match the other official plugins, but should be handled separately.
After that migration, Standard builds can consume a pinned, checksummed release
the same way they consume the other official instruments, and the expected set
would follow from the pins with no further edit. Minimal builds remain
unchanged.
