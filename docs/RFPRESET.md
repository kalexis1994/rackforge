# RackForge portable presets (`.rfpreset`)

`.rfpreset` is RackForge's portable, host-owned instrument preset format. It captures the complete opaque state of one plugin instance and can be transferred between Desktop, Android, Raspberry Pi/Web hosts, and the browser host.

## Version 1

The file is UTF-8 JSON with MIME type `application/vnd.rackforge.preset+json` and the extension `.rfpreset`.
RackForge suggests the portable filename `Plugin name - Preset name.rfpreset`, replacing only characters that are unsafe in a cross-platform filename.

```json
{
  "format": "org.rackforge.preset",
  "schema_version": 1,
  "exported_by": "RackForge 0.1.2",
  "exported_unix_ms": 1787539200000,
  "preset": {
    "schema_version": 1,
    "id": "warm-strings-0123456789ab",
    "name": "Warm Strings",
    "plugin_id": "org.rackforge.example",
    "created_unix_ms": 1787539100000,
    "updated_unix_ms": 1787539100000,
    "state": {
      "schema_version": 1,
      "plugin_id": "org.rackforge.example",
      "plugin_version": "1.4.0",
      "state_version": 3,
      "blob_sha256": "<64 lowercase hexadecimal characters>",
      "byte_length": 1234,
      "selected_sound_id": "example.program"
    }
  },
  "state_encoding": "base64",
  "state_base64": "<opaque plugin state>"
}
```

The plugin owns the bytes in `state_base64`. RackForge must never interpret or rewrite them.

## Compatibility

- `format` and the top-level `schema_version` identify the container contract.
- `plugin_id` must exactly match the destination plugin.
- `state_version` must match the state format declared by the installed plugin.
- `plugin_version` is recorded for diagnostics. A different plugin release may import the preset when it declares the same `state_version`.
- The selected program is a presentation hint. The complete opaque state remains authoritative.

Import is a two-stage operation: inspect, then apply. Inspection never mutates the preset library or the running plugin. It reports compatibility warnings and ID/name conflicts. Applying supports reject, replace, and keep-both conflict policies.

## Integrity and limits

RackForge recomputes the state identity from the plugin ID, plugin version, state version, and decoded bytes. The result must exactly match `blob_sha256`, `byte_length`, and the embedded immutable state reference.

Version 1 accepts states from 1 byte through 1 MiB and complete files up to 2 MiB. Unknown fields, unsupported schemas, invalid Base64, mismatched checksums, cross-plugin imports, and incompatible state versions are rejected before storage is changed.

## Persistence

Imported presets are stored in RackForge's host preset library, not inside a plugin package and not inside plugin private resources. Native hosts persist them under the RackForge data root. The pure browser host includes them in its IndexedDB-backed storage snapshot.
