# LIVE performance model

RackForge separates the object that sounds from the collections used to reach
it. `Rack` is the only directly playable object. `Song` and `Setlist` provide
ordered navigation without copying Rack state.

```text
Setlist -> Song -> Part -> Rack -> node graph -> plugins / child Racks
```

LIVE exposes three peer entry modes:

- `RACK`: all enabled Racks;
- `SONG`: enabled Songs, then their ordered Parts;
- `SETLIST`: enabled Setlists, flattened in Setlist/Song/Part order.

The user never has to create or enter a Setlist to play a Rack. Each mode keeps
its own last activated location. Moving the surface cursor changes only focus;
`OK` publishes an activation command.

## Ownership

RackForge owns Rack, Song, Setlist and LIVE-context documents. Plugins own their
programs and opaque state. A Rack Slot hosts a plugin and owns only host-level
routing/mix properties. The plugin returns an opaque PLAY-state ID so the Slot
can restore the selection later; RackForge never interprets or presents that ID
as a program.

Selecting a Slot's plugin embeds that plugin's regular PLAY surface in Slot
context. The host must not replace it with a second, simplified picker. A PLAY
selection updates the Slot's opaque state and previews the complete Rack draft,
not just that Slot, while navigation, collections and labels remain owned by
the plugin.

A Slot currently contains a name, plugin ID, optional opaque plugin-state ID,
enabled state, MIDI input channel, MIDI output route, audio output bus, level
and pan. MIDI and audio destinations are separate. Surfaces should expose only
the routes supported by the selected plugin's declared capabilities.
New Slots and legacy documents without an explicit channel default to `OMNI`;
channels 1–16 are explicit user-selectable filters.

The portable contracts live in `rackforge-performance-api` and contain no ALSA,
USB, Raspberry Pi or controller-model details. Documents use stable typed IDs,
explicit schema versions and bounded collections.

## Rack graph contract

Rack graph schema v2 makes routing explicit while keeping plugin state in its
Slot. Typed nodes represent MIDI/audio inputs, plugin Slots, child Racks and
MIDI/audio outputs. Typed edges connect stable input/output port IDs. Node
positions and canvas labels are presentation metadata and never participate in
audio compilation.

Labels can be notes or section bounds. Their position, size and color tone are
portable so an arrangement authored on Desktop remains understandable on other
platforms. The current zoom and pan viewport is intentionally device-local:
restoring a Desktop viewport on a phone would be actively unhelpful.

Flat Rack documents are graph schema v1 implicitly. Resolving one generates a
deterministic v2 graph in memory with one MIDI input, one node per Slot and
output nodes for the Slot routes. The new graph editor materializes that graph
when the user saves it; merely starting RackForge never rewrites existing
documents. Validation rejects
missing Slots or nodes, invalid ports, duplicate connections, feedback cycles,
dangling child Rack references and recursive Rack dependencies before runtime.

## Persistence

Global performance objects are stored below the RackForge data root:

```text
data/performance/
  racks/<rack-id>.json
  songs/<song-id>.json
  setlists/<setlist-id>.json
```

Every document is a regular bounded JSON file. The complete library is rejected
if it contains duplicate IDs, dangling references or invalid documents; Core
never publishes a partially loaded performance graph.

The stable LIVE context is checkpointed separately with the session. It stores
the chosen browse mode, one location per mode, the active location and the
resolved active Rack ID. Cursor-only movement remains local to the surface.

## Activation transaction

Core resolves a `LiveLocation` to one Rack, verifies that its plugin graph can
be executed by the current engine, asks the audio thread to apply it and only
then publishes session events. A rejected Rack leaves both audio and session
state unchanged.

The current engine creates one independent plugin instance per enabled Slot,
routes MIDI to every matching instance, applies Slot level/pan, and sums the
voices before the RackForge master stage. Rack replacement is transactional:
all candidate instances must load and activate before the previous Rack is
released. The Raspberry Pi 4 profile currently admits up to eight simultaneous
enabled Slots; the persisted contract remains independent of that platform
capacity.

## Bootstrap and evolution

An installation without performance documents imports the plugin's current
PLAY state as one Rack Slot, one single-Part Song and one default Setlist. The bootstrap
uses create-only atomic publication and never overwrites an existing library.

## Configuration transactions

`CONFIG` exposes three peer editors: `RACKS`, `SONGS` and `SETLISTS`. On a
LITTLE surface they support:

- Rack name, ordered Slots, enabled state, save and delete;
- Slot name, plugin-owned PLAY selection, MIDI/audio routes, level and pan;
- ordered Song Parts with independent names and Rack references;
- ordered Setlist entries with Song references;
- create, rename, reorder, enable, save and guarded deletion.

Editing happens in a surface-local draft. `SAVE` sends one complete typed
document together with the library revision that was used to create the draft.
Core clones and validates the complete candidate graph before changing disk.
It rejects stale revisions, dangling references, invalid collection sizes and
deletion of objects used by the active LIVE location.

The revision is the SHA-256 digest of the canonical serialized library, so it
survives restart without a separately synchronized counter. Publication uses a
temporary file, file `fsync`, atomic rename and directory `fsync`; the in-memory
library changes only after persistence succeeds. A failed or conflicting save
leaves the surface draft available.

LITTLE and WEB are views over this one authoritative transaction contract;
neither owns an alternate format or may silently overwrite the other.
