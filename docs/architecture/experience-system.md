# RackForge experience system

RackForge aims for a calm, continuous and deterministic interface. "Smooth"
does not mean animating every change: audio remains immediate, the current
musical context remains visible, and motion only explains a relationship or
confirms direct manipulation.

## 2026-08-27 baseline

The initial audit found that the visual language was present but not governed:

- `App.tsx` held about 5,900 lines and `LivePage.tsx` about 2,600;
- the main stylesheet exceeded 9,000 lines;
- 33 transition declarations, 15 animations and 41 responsive media rules used
  several unrelated timings, curves and breakpoints;
- tactile feedback created temporary viewport-positioned ripple elements;
- LITTLE resent body, header and footer with two 20 ms pauses for a screen
  update, and its six-second heartbeat repainted an unchanged display.

This baseline is intentionally recorded before migration. File size is not a
quality metric by itself, but the concentration makes behavior harder to test,
reuse and change without regressions.

## Product invariants

1. UI work never blocks the real-time audio path.
2. Every command reaches one terminal state: success, error or cancelled.
3. Revalidation preserves the last valid content instead of flashing empty UI.
4. A drag, scroll and long press cannot accidentally complete as a tap.
5. Web and LITTLE share semantic state and commands, not screen geometry.
6. Reduced motion changes presentation without removing state feedback.
7. Disconnection keeps stable identities and restores the last confirmed view.

## Experience budgets

The Web client records a small bounded set of local, non-identifying samples.
They are visible on the Diagnostics page and never leave the device.

| Measurement | Initial p95 target |
| --- | ---: |
| Pointer-to-feedback | 50 ms |
| Warm route-to-ready paint | 250 ms |
| Continuous Web frame | 16.7 ms |
| LITTLE button feedback | 80 ms |
| LITTLE screen commit | 150 ms |
| LITTLE restoration after MIDI is ready | 1 s |

These are release targets, not constants hidden in feature code. Measurements
must establish a platform baseline before a target is tightened.

## Interaction states

Host-owned controls use the same vocabulary:

```text
idle -> focused/hovered -> pressed -> pending -> success/error
```

`disabled`, `disconnected` and `stale` are explicit states. Pending operations
carry an identity so an older response cannot clear a newer operation. Cancel
only appears when work can actually be cancelled.

## Motion vocabulary

- instant: 80 ms, local feedback;
- fast: 140 ms, small entrances and exits;
- standard: 220 ms, navigation and disclosure;
- emphasized: 320 ms, spatial transitions such as a sheet;
- exit: 160 ms, leaving without making the user wait.

Components use the tokens in `web/src/design/tokens.css`. Continuous gestures
update transforms through the animation frame, while audio parameters travel
through their real-time parameter path without UI easing.

Route continuity is an opacity-only entrance on the persistent page surface.
Routes are not keyed or remounted for animation, and transforms are avoided
because they would temporarily move fixed dialogs, graph overlays and plugin
surfaces. The transition is skipped on initial render, in a hidden document,
on immersive performance surfaces and when reduced motion is requested.

## Asynchronous continuity

Initial loading may own a surface, but a refresh never replaces valid content
with an empty loader. Shared asynchronous boundaries keep the last confirmed
content visible and place a compact pending or error notice above it. Plugin
operations expose their pending state on the affected card, while success and
failure notices float without changing page geometry. Independent resources,
such as audio plugins and controller packages, load independently rather than
forming a UI waterfall.

## Responsive policy

New components respond to their container and input capabilities. The shared
categories are compact, regular and wide, refined by short viewport, coarse
pointer and safe-area insets. Device model names must not decide layout.

Touch Controller has two presentation states, not one state per orientation:

- `docked` is the default for desktops, portrait phones and tablets in either
  orientation;
- `immersive` is reserved for a touch-capable landscape viewport no taller
  than 600 CSS pixels, no wider than 1,200 CSS pixels and at least 7:4 wide.

The height and aspect requirements prevent a landscape tablet from being
treated as a phone merely because its width is greater than its height.

## LITTLE compositor direction

The LITTLE semantic runtime remains controller-independent. Its transport will
evolve toward revisioned screen snapshots, partial diffs, priority queues and
latest-wins coalescing for continuous controls. Heartbeats verify health without
repainting an unchanged display. A reconnect sends one complete authoritative
snapshot before incremental updates resume.

## Qualification

Web qualification adds visual regression, focus/keyboard, reduced-motion,
safe-area, overflow and gesture arbitration tests. LITTLE qualification adds
golden screens, delayed/lost ACKs, disconnects during commands, reordered input
and repeated-gesture soak tests.

The migration is incremental: PLAY, Plugin Manager, Settings, LIVE Perform,
graph editors and finally the remaining secondary flows.
