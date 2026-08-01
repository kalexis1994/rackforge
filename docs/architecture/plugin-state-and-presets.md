# Plugin state and RackForge presets

RackForge treats a plugin's current state as an opaque, versioned snapshot. The
host never reconstructs state from parameters and never inspects plugin-owned
variables. A plugin declaring the `state` capability must make `save_state` and
`load_state` a complete round trip for every user-visible setting that affects
its behavior.

## Three different objects

- A native program belongs to the plugin. Selecting it mutates an instance.
- A RackForge preset is a named, reusable copy of a complete instance state.
- A Rack Slot owns a private state snapshot. Loading a preset copies its state
  into the Slot; it does not create a permanent link.

Changing or deleting a preset cannot change an existing Rack. Slot edits do not
modify the source preset unless the user explicitly saves a new snapshot under
that preset name.

## Storage model

State payloads are immutable content-addressed blobs under `data/states/blobs`.
The SHA-256 covers the plugin id, plugin version, state schema version and opaque
bytes. Rack and preset documents contain a validated `PluginStateReference`,
not the bytes themselves.

Named host presets are stored under `data/states/presets/<plugin-id>`. Metadata
includes the owning plugin, plugin and state versions, timestamps and the state
reference. Catalog requests are always scoped by plugin id.

Writes use a temporary file, file synchronization and atomic publication. Reads
reject symlinks, unexpected file types, oversized payloads, length mismatches
and checksum mismatches. The current payload limit is 1 MiB.

## Runtime lifecycle

```text
plugin instance --save_state--> opaque bytes --hash/write--> state reference
state reference --verify/read--> opaque bytes --load_state--> plugin instance
```

The audio callback never performs filesystem I/O. State capture and restore are
control-plane operations executed between render blocks. Rack activation reads
and verifies blobs before asking the audio engine to construct Slot voices.

When an old Rack contains a native program id, RackForge creates a temporary
plugin instance, loads that program, exports a complete state and atomically
rewrites the Rack with the resulting reference. New Rack saves materialize the
state before the performance document is committed.

## Compatibility contract

Every reference records `plugin_id`, semantic `plugin_version` and the plugin's
integer `state_version`. `load_state` remains the final authority for validation
and migration: a plugin may accept older versions or reject an incompatible
payload without changing the running instance.

A future native-format adapter (CLAP, VST3 or Audio Unit) must use that format's
state stream/chunk API. Copying process memory is explicitly unsupported because
raw memory contains pointers and platform-specific runtime state.

Plugins with incomplete serialization must not declare the `state` capability.
Adapter-level `partial` state reporting and explicit external-resource manifests
are reserved extensions; RackForge will not silently present a partial snapshot
as a complete preset.
