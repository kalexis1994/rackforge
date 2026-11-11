# Controller packages, schema 2

Schema 2 redefines the `.rfcontroller` around one idea: every controller is a
`.rfcontroller`, however little RackForge knows about it. A player who plugs
in an M-Audio keyboard and names its knobs in the editor has made one; so has
the Arturia KeyLab package with its display driver. What differs is how many
layers the package fills, not which format it uses.

[Controller packages](controller-plugins.md) describes the schema 1 runtime
this builds on. Schema 1 packages keep loading unchanged.

## Why

Schema 1 describes mappings, not hardware. A semantic role, a host action and a
host control each repeat the MIDI message they listen to, and a knob that
carries no mapping does not exist at all. That leaves nothing to show a player
who wants to assign a knob themselves: RackForge cannot list the controls of a
keyboard it only knows through three role bindings. And the only way to
describe a controller is to write a package by hand.

Schema 2 puts the physical controls first. Everything else refers to them.

## Layers

| Layer | Required | Holds | Needs |
| --- | --- | --- | --- |
| 1. Identity | yes | id, name, vendor, how the device is recognised | MIDI input |
| 2. Inputs | no | every physical control: id, name, kind, the message it sends | MIDI input |
| 3. Meanings | no | semantic roles, host controls and host actions, by input | MIDI input |
| 4. Feedback | no | messages sent to the device: on connect, LEDs | MIDI output, SysEx |
| 5. Driver | no | an executable for displays, LITTLE and vendor protocols | trust |

A package fills layers from the top. A player's own M-Audio is layers 1 and 2;
a profile RackForge publishes from the vendor's MIDI implementation chart adds
3; a controller that must be put into DAW mode adds 4; the KeyLab fills all
five. Permissions follow from the layers present: without layer 4 a package
never sends MIDI, and without layer 5 it never runs code.

### 1. Identity

```toml
schema_version = 2
kind = "controller"
id = "org.rackforge.m-audio-oxygen-49-mk5"
name = "Oxygen 49 (MK V)"
vendor = "M-Audio"
version = "1.0.0"
controller_api = "^1.1"

[[devices]]
id = "oxygen-49-mk5"

[[devices.endpoints]]
role = "performance_input"
name_contains = ["oxygen 49"]
exclude_contains = ["daw", "mcu"]

# Optional: the Universal SysEx Identity Reply, the most reliable way to tell
# models apart.
[devices.sysex_identity]
manufacturer = [0x00, 0x01, 0x05]
family = 0x0027
model = 0x0001
```

Endpoint names and USB identity work as in schema 1. `sysex_identity` is new:
the manufacturer ID (one byte, or three beginning with `0x00`), the 14-bit
family and model codes of the Identity Reply. It never replaces the endpoint
matcher, which stays the required positive identity.

When a device some package claims by its port name connects, the host sends
it the Identity Request (`F0 7E 7F 06 01 F7`) and waits 400 ms for the reply
on that input. The reply then decides:

- a package whose `sysex_identity` differs from the reply does not get the
  device, however well its name matches;
- of several packages that claim the same port name, the one whose identity
  matches gets it;
- a device that does not answer is bound by its name alone, as before.

A reply that arrives late binds the device again. The Controllers section
says when the device was recognised by its reply. The Pi's controller host,
the desktop and Android all ask; the KeyLab's driver speaks for the KeyLab.

`[runtime]` and `[permissions]` become optional. Left out, a package is
declarative and asks for MIDI input only. A declarative schema 2 package needs
at least one input and nothing more: named knobs with no roles are a complete
package, whose knobs the player maps.

### 2. Inputs

```toml
[[inputs]]
id = "knob-1"
name = "Knob 1"
kind = "knob"
group = "Knobs"
midi = { cc = 74, channel = 0 }

[[inputs]]
id = "button-1"
name = "Button 1"
kind = "button"
group = "Buttons"
midi = { cc = 20, channel = 0 }
button = { press = 127, release = 0 }

[[inputs]]
id = "pad-1"
name = "Pad 1"
kind = "pad"
group = "Pads"
midi = { note = 36, channel = 9 }

[[inputs]]
id = "encoder-1"
name = "Encoder 1"
kind = "encoder"
midi = { cc = 16, channel = 0 }
encoder = "relative_twos_complement"

[[inputs]]
id = "pitch"
name = "Pitch wheel"
kind = "wheel"
midi = { pitch_bend = true, channel = 0 }
```

| Kind | Messages | Notes |
| --- | --- | --- |
| `knob`, `fader` | CC | absolute, 0–127 |
| `encoder` | CC | endless; `encoder` names the encoding (`absolute`, `relative_twos_complement`, `relative_binary_offset`, `relative_sign_magnitude`) |
| `button` | CC or note | `button.press` / `button.release`; no `release` means the button reports only presses; `button.latching = true` when the hardware toggles by itself |
| `pad` | note | velocity-sensitive |
| `wheel` | pitch bend or CC | pitch bend carries 14 bits |
| `pedal` | CC | sustain, expression |

An input's `id` is stable across package versions: a player's mappings refer
to it. Two inputs never share a message. `group` only orders the editor's
list. Channels are zero-based, as everywhere in the controller API.

Keys are not inputs. A keyboard's keys are performance MIDI and stay out of
the list; a pad that sends notes is an input because the package says so.

### 3. Meanings

Roles and host actions name an input instead of repeating a MIDI message:

```toml
[[roles]]
input = "knob-1"
role = "synth.filter.cutoff"

[[roles]]
input = "fader-9"
role = "rackforge.master.level"

[[actions]]
input = "button-9"
target = "keyboard_parts"
```

A role is read absolutely unless it says `mode = "relative"`, which only an
encoder reporting a position may: the same endless encoder can move a value
to where it points or by the distance it turns, so the role, not the
hardware, decides. `invert = true` flips a role's direction. An input carries at most one meaning, and a role is given to one
input. Schema 1's `host_controls` has no schema 2 form: RackForge's master
level and pan are the roles `rackforge.master.level` and `rackforge.master.pan`.
A schema 2 package that declares `host_controls`, `host_actions` or
`semantic_profile` is refused, and a schema 1 package that declares any schema
2 field is refused too.

RackForge lowers these declarations into the schema 1 runtime structures
(`ControllerPackageManifest::profile`), so every host that runs schema 1 runs
schema 2 without a change. The lowered semantic profile's source id is the
top-level `source_id` when the package declares one, and
`controller.<package id>` otherwise. A package that moves from schema 1 keeps
its old source id, so the links a player learnt on its controls stay
attached.

For now roles accept inputs that send a control change (knobs, faders,
pedals, wheels on a CC, encoders reporting a position), and host actions
accept buttons that send a control change and report their release: the
runtime does not yet match notes, pitch bend or relative encodings. A package
may still declare those inputs; the player's mappings and the editor use them.

### 4. Feedback

Messages a declarative package sends when its controller connects, without
code -- typically the SysEx that puts a keyboard in the mode the package
describes:

```toml
[permissions]
midi_input = true
midi_output = true
sysex = true

[[on_connect]]
message = "F0 00 20 6B 7F 42 02 00 40 50 01 F7"

[[on_connect]]
message = "B0 7F 00"
```

The rules for the messages:

- Each is one complete message in hex bytes: a channel message, or one SysEx
  message from `F0` to `F7` with 7-bit data. There are at most 32, of at most
  1 KiB each.
- System common and realtime messages (clock, start, stop, reset) are the
  host's to send, never a package's.
- The permissions follow from the messages exactly. `midi_output` is asked for
  if and only if there are messages, and `sysex` if and only if one of them is
  SysEx. The player therefore reads the request as what the package will do.
- A driver sends its own messages, so `on_connect` belongs to declarative
  packages only.

Nothing is sent until the player allows it. The Controllers section shows
the messages and an Allow button, and the answer is kept in the package's
install record for its active version. A new version asks again, because what
it sends may have changed. Packages RackForge ships (official and certified
trust) are allowed as installed.

Hosts send the messages once per connection, after the Identity Request had
its answer or its 400 ms, with 20 ms between messages. A package allowed while
its controller is connected sends at once. The route is
`PUT /api/v1/controllers/{id}/output {"allow": true}` on every host that keeps
controllers.

LED states bound to inputs come later.

### 5. Driver

`[runtime] kind = "process-v1"` with its entrypoints, as in schema 1, for what
data cannot describe: LITTLE, displays, vendor protocols. A driver reports
the profile it serves; RackForge checks it against the lowered manifest.

## Compatibility

`schema_version = 1` keeps its contract. A schema 1 package has no input list:
the editor shows the messages it binds as unnamed inputs. Hosts older than
schema 2 refuse a schema 2 package by its version, before reading anything
else.

## Controllers made by the player

The editor writes schema 2 packages. A player's M-Audio becomes a declarative
package with an id under `user.` (`user.m-audio-oxygen-49`), installed in the
ordinary controller store, exportable as a `.rfcontroller` and shareable. To
change a package RackForge ships, the player duplicates it into a `user.`
copy; the copy stops following RackForge's updates. An overlay that follows
them is possible later and is not needed first.

## The player's mappings

Mappings are not part of a package. A package is immutable and RackForge
updates it; a mapping belongs to the player. One map per controller holds its
mappings for every plugin, in the data root:

```text
<data-root>/controller-maps/<controller-id>.json
```

The same map travels as an `.rfmap` file (`format = "org.rackforge.map"`),
exported and imported whole, like `.rfpreset` and `.rflive`. The types are
`ControllerMap` and `RfMapFile` in `rackforge-midi-api`; the store is
`rackforge-core::controller_map_store`.

Each mapping names an input -- its id and name, and a copy of the message it
sends, so the map still works where the package is missing -- a plugin
parameter by its stable `id` (never by index: indices may change between
plugin versions) and a mode. Within a plugin an input carries one mapping,
and two mappings never listen to the same message. A mapping whose parameter
a new plugin version no longer has stays, reported pending, and costs the
others nothing.

The mode is part of the link itself (`ParameterLink::mode`), so a link learnt
in a session can use one as well, and the Pi and the desktop run it through
the same compiled link.

The mode says what the input does to the parameter:

| Input | Mode | Does |
| --- | --- | --- |
| button, pad | **Set** | sets one value |
| | **Toggle** | alternates between two values |
| | **Cycle** | steps through chosen values, A → B → C → A |
| | **Hold** | one value while pressed, another on release |
| | **Step** | one step up, or one step down |
| | **Trigger** | fires a trigger parameter |
| knob, fader, pedal, wheel | **Range** | the travel spans a minimum to a maximum, optionally inverted |
| | **Zones** | the travel is divided among a choice parameter's values |
| encoder | **Relative** | moves the value by the distance turned, with a sensitivity |

Range and Zones pick up the parameter like every absolute control: nothing
moves until the control crosses the parameter's value. A mapped button or pad
consumes its message, so a pad that switches the Leslie does not also play a
note; a mapped knob passes its message through, as links do today.

The same input may carry one mapping per plugin; the one that applies is the
one for the plugin playing. Precedence, highest first:

1. a Rack's or Song Part's own links (LIVE, saved with the Rack);
2. the player's mapping for this controller and plugin;
3. the package's semantic roles, joined with the roles the plugin publishes.

MIDI Learn writes into the player's mapping for the controller and plugin in
use, so a control learnt once stays learnt.

## The Controllers section

A section of its own holds all of it:

- a device selector: connected controllers and known ones;
- the controller's inputs on the left, grouped, each lighting when its control
  moves, marked when it has a mapping and marked differently when it has one
  for the plugin playing;
- for a controller RackForge does not know, an empty list that fills as the
  player moves controls, each named as it appears;
- selecting an input opens its mappings: every plugin it controls, and adding
  one means choosing a plugin, then one of its parameters (grouped, with
  search), then the mode.

The package's roles show as the standard assignment an input has until the
player replaces it; reserved inputs, such as the KeyLab's LITTLE keys, show
locked. Lighting an input needs a host feed of incoming controller messages
while the section is open, in addition to Learn's one-shot capture.

## Steps

1. Schema 2 manifests: identity, inputs and meanings, lowered to the schema 1
   runtime. The generic example moves to schema 2. *(Done.)*
2. The KeyLab package moves to schema 2; its driver reports the lowered
   profile. *(Done.)*
3. The player's mappings and modes in Core, applied in PLAY; MIDI Learn writes
   to them. *(Done on the Pi, the desktop and Android. The browser demo and
   the VST3 editor show controllers and keep no maps.)*
4. The Controllers section, with the live input feed; packages made from it.
   *(Done.)*
5. Published profiles for common controllers, from their vendors' charts.
6. Feedback (layer 4) and SysEx identity matching. *(Done for connect
   messages; LED states bound to inputs remain.)*
7. LIVE: which slot a controller-and-plugin mapping follows, and Rack links.
