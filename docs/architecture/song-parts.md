# Song Parts and performance graphs

## Decision

RackForge uses four distinct performance concepts:

```text
Setlist -> Song -> ordered Song Parts -> graph -> instruments / child Racks
                                             -> audio output
```

- A **Rack** is a reusable graph. It is useful across Songs and can also be
  played directly.
- A **Song** is an ordered navigation document. It does not produce audio by
  itself.
- A **Song Part** is an activatable scene and owns one complete graph.
- A **Setlist** is an ordered list of Songs for a performance.

This is deliberately a hybrid of an ordered Song editor and a general graph
engine. Musicians see Intro, Verse, Chorus and Solo in a predictable order,
while each Part keeps the routing power of a rack-of-racks system.

## What sounds inside a Part

A Part is a graph, not an implicit group. A node sounds only when it belongs to
a valid signal path:

```text
MIDI Input -> instrument or Rack -> Audio Output
```

Several valid paths may be active at once. This naturally supports:

- layers: two instruments receive the same input and sound together;
- splits: MIDI cable filters route different note ranges;
- channel routing: different MIDI channels reach different destinations;
- reusable sections: one Part can contain several child Racks;
- intentionally silent or disconnected nodes while authoring.

Merely placing a node on the canvas never makes it audible. Connectivity,
enabled state and MIDI transformations determine the result.

## Editing model

The Song editor has two coordinated regions:

1. an ordered Part navigator for creating, naming, moving and deleting Parts;
2. the shared RackForge graph editor for the selected Part.

The graph editor must be the same implementation used by Racks. Instrument
editing, MIDI cable editing, labels, zoom, pan and responsive overlays therefore
behave consistently on Desktop, Android and the hosted Raspberry Pi UI.

Changing Parts in LIVE is an atomic graph replacement: Core validates and
prepares every required plugin instance before replacing the previous graph.
A failure leaves the currently sounding Part untouched.

## Navigation and controller mapping

Songs define order, not hardware bindings. RackForge exposes semantic actions:

- `ActivateSongPart(part_id)`
- `NextSongPart`
- `PreviousSongPart`

An `.rfcontroller` profile maps physical buttons, pads or MIDI CC messages to
those actions. The Song document remains portable and does not embed an Arturia,
Android or Raspberry Pi specific MIDI mapping.

## Persistence and compatibility

Graph-backed Parts persist their Slots and graph inside the Song document.
Plugin programs remain plugin-owned; a Slot stores an opaque RackForge preset
state plus host routing and mix properties.

Legacy Parts contain only `rack_id`. They remain valid and playable. When an
old Part is first edited with the graph editor, the UI materializes this graph:

```text
MIDI Input -> referenced Rack -> Audio Output
```

Saving writes the explicit Part graph. Startup does not eagerly rewrite user
documents.

## Runtime identity

Core compiles a graph-backed Part as a synthetic root Rack whose stable runtime
ID is the Song Part ID. Child Rack references still resolve through the shared
performance library. This reuses the same validation, graph compiler, audio
transaction and voice limits on every platform without copying DSP behavior
into the UI.

## Evolution

The first implementation uses the existing parallel-instrument graph compiler.
The document model already permits richer graph nodes. Effects, audio inputs,
crossfades and Part transition policies can be added without changing the
Song/Part ownership boundary.

