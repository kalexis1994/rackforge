# rackforge-session-api

Pure, serializable, platform-independent RackForge session model.

The session is the shared source of truth for LITTLE, WEB, desktop, Android, and
future surfaces. Inputs produce `SessionCommand` values; accepted changes
produce `SessionEvent` values with monotonic revisions.

Every command carries a stable `client_id` and `command_id`, allowing
multiple surfaces to correlate results without collisions and reconnect from a
known revision.

The crate contains no ALSA, USB, controller, socket, dynamic-library, or UI
implementation.
