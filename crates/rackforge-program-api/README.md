# rackforge-program-api

Serializable, platform-independent contracts for programs, edit requests, and
plugin-prepared documents.

The plugin owns and validates its payload. RackForge validates the common
envelope, controls draft/audition lifecycle, and persists documents atomically
inside the plugin namespace.

Visual editing uses a versioned declarative tree. Pages contain subpages and
typed fields such as `toggle`, `number`, `choice`, and `sound`. Numeric
values travel as integers with explicit decimal precision to avoid
cross-platform floating-point display differences.

A surface sends only `ProgramFieldEditRequest { field_id, value }`. The field
ID is opaque to the host; only the plugin maps it to its payload, applies the
change, validates invariants, and returns a prepared program.
