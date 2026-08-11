# rackforge-ui

Hardware-independent UI framework for RackForge. It receives logical inputs,
manages focus and components, and produces styled cell frames without knowing
MIDI, SysEx, KeyLab, or the audio engine.

Core concepts:

- `Input`: seven physical controls and their short/long gestures.
- `NavigationAction`: reusable navigation semantics.
- `Style`: semantic visual roles.
- `Component`: stable identity, state, event handling, and rendering.
- `EditorState`: OK starts/confirms editing; BACK cancels an active edit and
  requests exit only while navigating.
- `TextFallback`: deterministic rendering for displays without advanced style.

The initial palette is intentionally compact: normal, focused, pressed,
disabled, warning, and error. Components include buttons, carousels, text and
secret editors, confirmation dialogs, spinners, and typed value carousels.

The crate contains no controller transport and can be tested without hardware.
