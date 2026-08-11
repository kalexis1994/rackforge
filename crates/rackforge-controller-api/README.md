# rackforge-controller-api

Pure, versioned contracts that separate musical MIDI input from a privileged
control surface.

- Unknown MIDI ports may feed notes, CC, pitch, and pressure to the engine.
- Only a registered `ControllerDriver` may open display/SysEx output.
- Layouts are explicit contracts and are never inferred from resolution or
  control count.
- Negotiation uses only controller implementations and plugin views declared
  in their manifests.
- A profile may reserve exact physical messages for typed host targets such as
  `master_level`, `master_pan`, and `keyboard_parts`. Core consumes those
  messages before plugin routing.
- Trust is assigned by installation state, not claimed by a package.

`little@1` defines a header, two body lines, four soft keys, an encoder, and
Previous/Next/Confirm/Back actions. A `medium@1` controller does not gain
LITTLE compatibility automatically; it must ship and test an implementation.
