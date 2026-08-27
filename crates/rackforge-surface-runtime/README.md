# rackforge-surface-runtime

Host-owned state, navigation, and composition for the `little@1` surface,
without MIDI, USB, SysEx, or controller-model knowledge.

The runtime receives `rackforge_ui::Input`, updates host/plugin view state,
emits typed `MenuCommand` values, and renders a logical `Screen` containing
a header, two body lines, and four footer keys. A physical driver only translates
device messages into logical input and serializes the resulting screen.

PLAY, LIVE, plugin loading, programs, performance editing, audio, WEB, and Wi-Fi
all project authoritative Core state. Long BACK and the emergency HOME chord
remain host-owned.

This separation lets a new `.rfcontroller` reuse the same menus without
copying plugin-specific behavior.

## Screen delivery

`ScreenCompositor` turns logical screens into revisioned `ScreenUpdate` values.
Each update names the header, body and footer regions that changed and carries
an explicit background, interactive or immediate priority. Repeated screens are
suppressed and pending bursts use latest-wins coalescing while retaining the
highest priority in the burst.

`ScreenMailbox` is the thread-safe, bounded handoff used by hosts. It never grows
an event queue: producers publish the newest desired screen and the transport
takes one authoritative update when it can send. A transport must call
`invalidate_delivery()` after a failed write or physical reconnect so its next
update contains a complete screen.
