# MIDI routing architecture

Status: foundation accepted, schema `1`

## Goal

RackForge owns physical MIDI input, routing, splits, layers and transforms.
Plugins own the musical meaning of MIDI after a route reaches one of their
input buses. The same routing model is used by PLAY and LIVE, so a PLAY setup
can later be embedded in a LIVE rack without changing plugin state.

The routing contract is platform-neutral. ALSA, CoreMIDI, Windows MIDI and
Android MIDI are backends which discover sources and feed the same runtime
router.

## Ownership boundary

RackForge owns:

- physical and virtual MIDI source discovery;
- selecting the primary controller;
- input channel filters;
- key and velocity splits;
- layering one event into multiple destinations;
- transpose and destination-channel transforms;
- reserved host controls;
- controller-state tracking;
- note ownership and stuck-note prevention.

A plugin owns:

- whether it is single-part or multitimbral;
- its named MIDI input buses;
- pitch-bend range and interpretation;
- sustain, modulation and expression behaviour;
- plugin-program layers and their sound-design ranges;
- program changes, if it declares support for them.

A host key split decides where an event goes. A plugin-program key range
decides how the receiving instrument sounds. These are deliberately separate.

## Modes

### PLAY

PLAY is a host-generated rack containing the selected plugin instance and a
default MIDI route:

- source: `primary`;
- input channel: `omni`;
- destination: selected plugin, bus `main`;
- output channel: `auto`.

`auto` maps all channel voice messages to channel 1 for a single-part plugin.
For a multitimbral plugin it preserves the source channel.

Advanced PLAY routing is allowed, but it uses the same route objects as LIVE.

### LIVE

The intended hierarchy is:

```text
Setlist
  Song
    Part
      Rack
        Plugin instances
        MIDI routes
```

Every route contains:

- stable route ID and enabled state;
- source selector;
- input channel selector;
- message, key and velocity filters;
- transpose and output-channel transform;
- destination plugin instance and input bus.

All matching routes are evaluated. This is what implements layers. Key ranges
implement splits.

## Channel representation

User interfaces and persisted documents use channels `1..16`. MIDI bytes and
runtime indexing use `0..15`. Conversion is explicit at the API boundary; raw
channel integers must not leak into route documents.

## Source identity

`MidiSourceId` is stable and persisted. A platform backend derives it from the
best durable identity it can obtain, such as USB topology plus vendor/product
identity or an explicit user alias.

`MidiSourceKey` is a compact runtime-only number. Backends resolve stable IDs
to keys during discovery. Only keys travel through the real-time callback and
lock-free/bounded queues. Runtime keys must never be saved.

Port display names are not identities. If a backend cannot distinguish two
devices, it must surface the ambiguity instead of silently binding a saved
route to the wrong controller.

## Event order

The processing order is:

```text
backend input
  -> validate channel-voice packet
  -> attach runtime source key
  -> intercept reserved host controls
  -> update state for source + input channel
  -> match every active route
  -> key/velocity/message filters
  -> transpose and output-channel transform
  -> note ownership ledger
  -> destination plugin input bus
```

System realtime and SysEx messages do not enter this router. Controller
display SysEx remains in the controller-driver subsystem.

## Note safety

Filtering note-off by release velocity is forbidden. During the full LIVE
implementation, RackForge keeps a note ownership ledger keyed by:

```text
(source key, input channel, input note, note generation)
```

The value is every destination/channel/transposed-note that received the
matching note-on. Note-off is sent to those exact owners even if routing is
edited meanwhile.

Disabling a route, changing a rack, losing a source or stopping a plugin must
flush its owned notes and relevant pedal state. A global emergency action sends
all-notes-off, resets controller state and clears the ledger.

## Controller state

CC, pitch bend, channel pressure, sustain and related state are tracked per
source and input channel. State replay after preset/program changes is routed
through the same route graph; sources must never overwrite each other's cached
state merely because they use the same MIDI channel.

Host-reserved controls are intercepted before plugin routes. A future binding
may be source-specific; channel plus CC alone is insufficient when multiple
controllers are connected.

## Plugin contract evolution

Plugins declare:

- channel model: `single_part` or `multi_part`;
- named MIDI input buses, with `main` required for instruments;
- program-change policy: `ignore` or `plugin_defined`.

Detailed supported-message capabilities can be added without changing route
documents; schema 1 routes already identify message kinds independently.

Plugins do not discover hardware, open MIDI ports, or save host routes. They
receive normalized MIDI packets for a declared input bus.

## Rollout

1. Introduce the versioned routing types, validation and pure allocation-free
   compiled router.
2. Preserve source identity at the current Core MIDI ingress without changing
   audible behaviour.
3. Add plugin channel/input-bus declarations, then PLAY's generated default
   route.
4. Store route graphs in sessions and expose editing through LITTLE and WEB.
5. Add multiple plugin instances, audio graph mixing and the note ownership
   ledger.
6. Add LIVE Racks, Songs, Parts and Setlists through the shared performance API.

Each phase must preserve the current one-plugin PLAY path until its replacement
has tests and an explicit migration.
