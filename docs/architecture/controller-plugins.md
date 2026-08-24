# Controller packages (`.rfcontroller`)

This document describes the current architecture. Historical implementation
notes live in [Controller package history](../history/controller-plugins-2026-08.md).

## Runtime matrix

| Runtime | Windows Desktop | Linux x86-64 | Raspberry Pi | Android | Browser | Intended use |
| --- | --- | --- | --- | --- | --- | --- |
| `declarative-v1` | Host-owned | Host-owned | Host-owned | Host-owned | Import pending | Ordinary MIDI controls, semantic mappings, and host actions |
| `process-v1` | Supported | Supported | Supported | Prohibited | Prohibited | Displays, LEDs, SysEx, and vendor protocols |
| `wasm-v1` | Reserved | Reserved | Reserved | Reserved | Reserved | Future portable rich drivers |

`declarative-v1` is the community entry point. It is a TOML manifest with no
binary and no executable community code. A package matches MIDI inputs, maps CC
messages to RackForge's semantic vocabulary, and can declare host-owned controls
or actions. Windows, Linux x86-64, Raspberry Pi, and Android interpret the same
package; the pure browser host still needs persistent package import before it
can do so.

`process-v1` is for hardware such as the Arturia KeyLab that requires
bidirectional MIDI, SysEx, LITTLE, or LED feedback. Android cannot run binaries
from writable app storage and browsers cannot spawn processes, so it is not a
portable community runtime.

`wasm-v1` is reserved in the serialized contract but is not executed yet.
Validation and supervision report that explicitly.

## Declarative package contract

A declarative package must:

- use `runtime.kind = "declarative-v1"` and declare no entrypoints;
- request MIDI input only;
- declare at least one `surface_input` or `performance_input` matcher;
- declare no display surface, settings handler, SysEx, MIDI output, USB metadata,
  filesystem, network, raw USB, or firmware permission;
- contain at least one semantic mapping, host control, or host action;
- use non-overlapping MIDI bindings.

RackForge matches only enabled MIDI inputs. Matching is case-insensitive and
uses positive and negative endpoint-name rules. USB VID/PID may be supplied as
extra identity, but is optional because browser and Android MIDI APIs do not
expose it consistently.

If two enabled packages match the same input, RackForge reports an ambiguity and
activates neither. It never silently chooses a driver. A disconnect does not
delete mappings; the host resolves them again when the endpoint returns.

The reference package is
[`examples/controllers/generic-midi`](../../examples/controllers/generic-midi/README.md).

## Semantic controls and pass-through

The semantic profile translates physical CC messages into public roles such as
`synth.filter.cutoff`, `synth.envelope.amp.attack`, or
`rackforge.master.level`. It never names a plugin or parameter index.

Plugins independently publish roles they implement. RackForge validates and
compiles the connection against each plugin's public parameter schema. Runtime
instance identity keeps two slots of the same plugin independent.

MIDI remains pass-through by default. A declarative mapping observes a message
and applies its host or parameter meaning without silently removing the original
message from musical routing.

Current native-host coverage is intentionally explicit: Windows Desktop
interprets semantic mappings plus the existing master controls and host-action contract;
Android interprets semantic plugin mappings and RackForge master level/pan;
Linux x86-64 and Raspberry Pi register semantic plugin mappings through Core
without reserving or consuming their CC messages. General host-action dispatch
on Android and Linux is the next additive controller-API step; packages keep
those declarations, but the hosts do not pretend they executed an unsupported
action.

## Process packages

`process-v1` packages carry an executable per supported target. RackForge's
shared supervisor starts enabled drivers, supplies the control endpoint and
settings path, restarts failures with backoff, and closes their supervisor pipe
during shutdown. Community executables require explicit trust.

Process drivers own vendor protocols and translate them into the same public
session, surface, semantic-control, and host-binding contracts. They must restore
hardware state when the supervisor pipe closes.

The KeyLab driver is a production implementation, not a starter template. New
generic controllers should start with `declarative-v1`; reusable process-driver
helpers should replace copying its monolithic executable before another rich
hardware driver is encouraged.

## Store and lifecycle

Native hosts share the `PackageStore` under `<rackforge-root>/controllers`:

- immutable versions in `packages/<id>/<version>`;
- one active record in `active/<id>.json`;
- runtime settings in `state/<id>/settings.toml` when supported;
- trust and enabled state in the active record.

Install validation applies path, size, identity, API compatibility, and artifact
integrity checks before activation. Installing a declarative community package
does not grant code-execution permission.

## Ownership boundary

RackForge owns discovery, enabled-input policy, stable source identity, semantic
mapping, host actions, persistence, reconnection, and pass-through. A declarative
package supplies data only. A rich driver owns vendor-specific I/O but uses
RackForge's public contracts.

LITTLE, LEDs, display rendering, and SysEx are outside `declarative-v1`. They use
`process-v1` today and become portable when sandboxed `wasm-v1` is implemented.
