# rackforge-controller-api

Pure, versioned contracts that separate musical MIDI input from a privileged
control surface.

- Unknown MIDI ports may feed notes, CC, pitch, and pressure to the engine.
- Only a registered `ControllerDriver` may open display/SysEx output.
- Layout IDs describe semantic navigation contracts, not screen-size
  breakpoints. Every controller declares its physical viewport separately and
  projects the same model responsively.
- Negotiation uses the host surface contracts and the implementations declared
  by the controller. Plugins do not need to publish a LITTLE layout merely to
  appear in PLAY.
- A profile may reserve exact physical messages for typed host targets such as
  `master_level`, `master_pan`, and `keyboard_parts`. Core consumes those
  messages before plugin routing.
- Trust is assigned by installation state, not claimed by a package.

`little@1` defines the compact RackForge information hierarchy and
Previous/Next/Confirm/Back semantics. The Arturia reference implementation has
an 18-column header, two body rows and four soft keys, but another controller
may implement the same contract with a wider or narrower viewport. The surface
renderer chooses full or compact plugin identity, truncation and spacing from
that viewport; a new layout ID is needed only for a different interaction
model.
