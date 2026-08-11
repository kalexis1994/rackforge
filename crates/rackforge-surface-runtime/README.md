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
