# Controller packages

Physical controller integrations do not belong to Core or audio plugins. Each
integration is distributed as a self-contained `.rfcontroller` directory with
a validated `rackforge-controller.toml` manifest and target-specific artifacts.

The installed store keeps immutable versions:

```text
controllers/
├── packages/<controller-id>/<version>/
└── active/<controller-id>.toml
```

Installation validates archive limits, paths, identity, runtime API, artifacts,
and SHA-256 integrity before committing a package. The separate `active`
record selects a version and administrator-assigned trust level.

Installing an update never deletes the previous version. An administrator can
atomically reactivate a known-good version and restart the host. A package
cannot elevate its own trust through its manifest.

Controller packages declare endpoint matchers, certified layouts, reserved host
controls/actions, LED profiles, permissions, and artifacts. Unknown devices may
still provide musical MIDI but never receive SysEx or display access.

The Arturia KeyLab Essential mk3 package is the reference implementation.
