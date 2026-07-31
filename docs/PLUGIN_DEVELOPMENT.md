# External plugin development

RackForge hosts plugins; it does not own their engines, assets, or product
interfaces. Each production plugin lives in an independent repository and is
installed as a versioned `.rfplugin` package.

## Repository boundary

The RackForge repository owns the portable SDK contracts, plugin discovery,
native loader, MIDI/audio routing, session model, controller surfaces, and Web
shell. A plugin repository owns its DSP or sound engine, program model,
declarative `little@1` editor, static Web surfaces, tests, and packaging tools.

Runtime data is never part of a package:

```text
/home/kalex/rackforge/plugins/<package>/          immutable installed package
/home/kalex/rackforge/data/plugins/<plugin-id>/  banks and user documents
```

RackForge discovers behavior from the package manifest and runtime descriptors.
Host code must not branch on a plugin ID to draw menus or interpret plugin
state.

## SDK dependency

Until the SDK crates are published independently, plugin repositories pin the
RackForge Git revision they support. For adjacent local checkouts, Cargo source
patches may redirect those dependencies to the working SDK crates. Such patches
belong in ignored `.cargo/config.toml`; release metadata keeps the reproducible
Git pin.

Adding a serialized field that an older host cannot safely consume requires a
Plugin API minor revision. Existing plugins declaring an older minor remain
loadable. New packages declare the first host minor that provides every contract
they use.

## Package contract

A package is a directory whose name ends in `.rfplugin`:

```text
plugin-name-version.rfplugin/
├── rackforge-plugin.toml
├── lib/
│   └── <platform native library>
└── web/
    └── <static plugin-owned surfaces>
```

Sound banks, ROMs, credentials, caches, and saved programs are forbidden from
the package. The installer replaces only the immutable package directory and
preserves the plugin data directory.

## RF-DLS development checkout

The reference external plugin lives in the adjacent
`rackforge-plugin-rf-dls` repository. With both repositories checked out next
to one another:

```bash
cd ../rackforge-plugin-rf-dls
cp .cargo/config.toml.example .cargo/config.toml
cargo test --workspace
bash tools/build-package.sh
bash tools/install-package.sh ./rf-dls-0.1.0.rfplugin
```

RF-DLS owns the DLS engine and all plugin-specific views. RackForge receives a
generic preset catalog; its `editable` flag is the only fact the host needs to
offer the plugin's program editor for CUSTOM entries.
