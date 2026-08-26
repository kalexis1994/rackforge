# MIDI Parameter Links

RackForge owns MIDI-to-parameter mapping. Plugins publish parameter semantics
through their public parameter schema and may render those parameters however
they choose. A custom Web surface opts an element into the host menu with:

```html
data-rackforge-parameter-index="17"
```

The host intercepts right-click and touch long-press inside same-origin plugin
frames. The menu and editor are rendered outside the plugin frame, so plugins
do not inherit a RackForge React component, CSS class, or control shape.
Non-primary mouse presses are stopped during capture and never reach the
plugin control beneath the menu.

During a non-primary press RackForge adds the transient
`rackforge-context-press` class to the marked element. Custom surfaces that
use raw CSS `:active` for pressed animation must exclude or override that class
so the context-menu gesture never looks like a parameter activation. The host
removes the class on pointer release or frame cleanup.

The same menu provides **Reset to program**. RackForge resolves the public
parameter value from a fresh immutable state for the selected plugin sound and
applies only that parameter; all other live edits and MIDI Links are preserved.
When no sound is selected, the parameter schema default is the fallback.
After the canonical write, the host sends a `parameter_changed` bridge message
so a custom surface can redraw without reloading its frame or showing its
initial front-panel loading state again.

## Ownership and persistence

`ParameterLink` is session state, not plugin state. It contains a stable link
ID, target instance and parameter index, a persistent MIDI source ID, the MIDI
message and channel match, input inversion, and an explicit pass-through
policy. The source's display name is informational and is never its identity.

Links are recorded as ordinary session events and saved in the session
checkpoint. A missing MIDI source leaves its link pending. Reconnection
resolves the stable source ID to a new compact runtime key; it does not rewrite
the project or plugin state.

## Learn

Learn is a transient host observer. Beginning Learn clears stale observed
messages, then captures the next compatible CC, Pitch Bend, Note, Channel
Pressure, or Poly Pressure message. It only fills the editor draft. Apply is
the first operation that records a session event or replaces the runtime link
table; Cancel therefore has no project, session, or audio side effect.

By default Learn listens to every enabled MIDI input and replaces the draft's
source with the source it actually observed. The source selector remains
useful for manual mapping, editing an existing link, and retaining a link to a
currently disconnected device; it is not an implicit Learn filter.

## Real-time path

Hosts compile links against the plugin's public parameter schema before they
reach audio processing. Unknown, read-only, meter, non-finite, or out-of-domain
targets are rejected. The compiled mapper converts MIDI into the normal
`ParameterEventV1` path:

- float and integer values use the declared range and step;
- booleans use a midpoint threshold;
- enums select only declared choice values;
- triggers emit activation and release;
- bipolar ranges preserve both endpoints and the center;
- Pitch Bend uses all 14 bits and maps 8192 to the exact center.

Runtime tables and event buffers are prepared outside the audio callback. The
callback performs bounded, allocation-free matching. `pass_through` is the
default, so a mapping observes the message without silently removing it from
the instrument. `consume` is an explicit per-link opt-in.

Controller packages that own an exclusive operating-system MIDI endpoint use
their stable controller client ID as the MIDI source ID when forwarding
three-byte channel messages. CC, poly pressure and Pitch Bend therefore reach
Learn and the audio mapper with the same identity; they must not be sent
through a note-and-sustain-only touch-controller filter.

A controller package may explicitly reserve messages for its RackForge
surface or host controls. Those messages remain on the controller plane and
are not forwarded as performance MIDI. For the Arturia KeyLab Essential mk3,
this includes the four LITTLE soft keys and the package-declared master
controls. Unreserved faders, knobs, notes, pressure and Pitch Bend continue
through the shared MIDI source and remain available to Learn and Parameter
Links.

## Bundled controller updates

Official controllers shipped with RackForge use immutable content-derived
package versions. The host hashes the source manifest and the platform driver,
embeds the driver in the application artifact, and installs that exact version
before starting controller supervision. Rebuilding an unchanged driver keeps
the same package version; changing either the driver or manifest creates and
activates a new version without overwriting the previous package or requiring
a sidecar executable.

## RackForge Control Profile v1

Controller defaults use semantic roles, never plugin-specific parameter
numbers. A `.rfcontroller` maps a physical message to a stable meaning, while
the plugin parameter schema maps that meaning to one writable public
parameter. RackForge joins both declarations at runtime and emits the same
validated `ParameterEventV1` used by MIDI Learn and the Web parameter bridge.
When a semantic control moves, RackForge also publishes compact transient
feedback to LITTLE without consuming the MIDI message destined for the plugin.

For example, the bundled KeyLab profile declares CC 109 as
`synth.filter.cutoff`. A synth can opt into that default by adding this to its
parameter schema:

```json
{
  "schema_version": 2,
  "semantic_controls": [
    { "role": "synth.filter.cutoff", "parameter_index": 17 }
  ]
}
```

Parameter schema 1 remains supported for existing plugins. Publishing
`semantic_controls` requires schema 2 so an older host rejects the new contract
explicitly instead of silently interpreting a changed schema.

Schema 3 additionally allows `display_decimals` as presentation metadata for
compact host surfaces. It never changes parameter resolution or MIDI mapping.

These generated links are ephemeral defaults:

- they are not written into the session or opaque plugin state;
- they reconnect through the runtime MIDI source identity;
- they always pass the original MIDI message through;
- a user-created link for the same parameter or physical message wins;
- a plugin that does not publish a role receives no automatic mapping;
- RackForge-owned global roles are handled by the host and never compiled into
  plugin parameter links;
- reserved LITTLE action messages cannot also be semantic controls.

The v1 vocabulary is versioned independently from controller and plugin
package formats. Official roles include oscillator pulse width/sub/noise,
filter cutoff/resonance/envelope/LFO/key tracking, amplifier ADSR/level, LFO
rate/depth/delay, plugin output level, mixer level/pan, RackForge master
level/pan, modulation, expression, and sustain. Third-party extensions use namespaced identifiers such as
`vendor.example.filter.color`; official role meanings never change in place.

The bundled KeyLab uses its first eight DAW-preset faders for amplifier ADSR,
filter cutoff/resonance, and LFO rate/depth. Its first eight encoders control
oscillator pulse width/sub/noise, filter envelope/LFO/key tracking, LFO delay,
and amplifier level. Fader 9 publishes `rackforge.master.level`; encoder 9
publishes `rackforge.master.pan` with relative interpretation. They travel
through the same semantic profile rather than private Arturia bindings. LITTLE
buttons remain on the controller action plane.
