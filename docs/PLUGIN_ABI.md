# The `wasm-v1` plugin ABI

This is the contract between RackForge and a plugin, written for someone
implementing it directly. It is a plain WebAssembly ABI: exported functions
that take and return `i32`, `i64` and `f64`, and buffers the host reads and
writes inside the module's own linear memory. Nothing in it is specific to a
language.

`rackforge-plugin-sdk` is one implementation of this contract, for authors who
choose Rust. It is not the contract. A module that exports the symbols below —
written in C, Zig, TinyGo, AssemblyScript, or WebAssembly text by hand — is a
RackForge plugin, and the host cannot tell the difference.

For what surrounds the component — the package directory, the manifest,
branding and distribution — see [PLUGIN_DEVELOPMENT.md](PLUGIN_DEVELOPMENT.md).
This document covers only the `component.wasm` inside it.

## The module

A plugin is a WebAssembly module that:

- **exports a linear memory named `memory`**, and
- **imports nothing at all.**

The second rule is enforced, not advisory: a module carrying any import is
refused at compile time, WASI included. There is no host function to call, no
clock, no file, no randomness that arrives from outside. Everything a plugin
needs is either compiled into it or handed to it through the buffers below.
That is what lets the same bytes run on Windows, Linux, Android, a Raspberry
Pi and inside a browser tab, and what makes a misbehaving plugin unable to
reach past its own memory.

## Version handshake

```wat
(func (export "rackforge_abi_version") (result i32))
```

Called once, before anything else. Return one of:

| Value | Meaning |
| --- | --- |
| `0x0001_0001` | ABI v1.1 |
| `0x0001_0002` | ABI v1 (current) |

The host accepts that range inclusive and refuses anything outside it. Return
the highest version whose contract you actually implement; the difference is
described under [Optional extensions](#optional-extensions).

## Status codes

Every function that reports success returns `i32`:

| Value | Name | Meaning |
| --- | --- | --- |
| `0` | OK | The call succeeded. |
| `-1` | invalid argument | The host passed something out of range. |
| `-2` | unknown parameter | No parameter has that index. |
| `-3` | invalid state | Not prepared, or the feature is not implemented. |

Returning `-3` from a feature you do not implement is the supported way to
decline it. A plugin with no presets, no saved state and no external resources
returns `-3` from all seven of those functions and works perfectly.

Functions that produce data return a **non-negative byte count** on success
and a negative status on failure. There is no separate out-parameter.

## Memory: buffers, not allocation

**Nothing is allocated across the boundary.** The plugin publishes the address
and capacity of each buffer it owns; the host writes into the module's linear
memory at those addresses and then calls with counts. No pointer is ever
returned to the host to free, no host pointer is ever handed to the plugin, and
there is no `malloc` in the contract.

Each buffer is announced by two functions:

| Buffer | Address | Capacity | Element |
| --- | --- | --- | --- |
| Audio input | `rackforge_input_ptr` | `rackforge_capacity_input_samples` | `f32` |
| Audio output | `rackforge_output_ptr` | `rackforge_capacity_output_samples` | `f32` |
| MIDI 1.0 | `rackforge_midi_ptr` | `rackforge_capacity_midi_events` | 8 bytes |
| Parameters | `rackforge_parameter_ptr` | `rackforge_capacity_parameter_events` | 16 bytes |
| Transfer | `rackforge_transfer_ptr` | `rackforge_capacity_transfer_bytes` | 1 byte |

All of them return `i32`, are called after `rackforge_initialize`, and must
keep answering the same values for the life of the instance.

The host validates every one before use, and refuses the plugin if:

- the address is negative or not aligned to the element size;
- the buffer would extend past the end of linear memory;
- **two buffers overlap.**

Everything is **little-endian**, which is WebAssembly's byte order everywhere.

### Audio

Interleaved `f32`. A block of `frames` frames at `channels` channels occupies
`frames * channels` samples: frame 0 channel 0, frame 0 channel 1, frame 1
channel 0, and so on. The host writes the input buffer before the call and
reads the output buffer after it.

Capacities are what you can survive, not what you will get. Publish the largest
block you support; the host will call `rackforge_prepare` with a
`maximum_frames` no larger than that, and every `process` with `frames` no
larger than *that*.

### MIDI 1.0 event — 8 bytes

One little-endian `u64` per event:

| Bits | Field |
| --- | --- |
| 0–31 | `frame` — sample offset inside this block |
| 32–39 | `data[0]` — status byte |
| 40–47 | `data[1]` |
| 48–55 | `data[2]` |
| 56–63 | `length` — 1, 2 or 3 |

Events arrive ordered by `frame`, and every `frame` is `< frames`. This is how
a plugin plays a note at the sample it happened rather than at the start of the
block. SysEx never arrives here — it travels on the control plane, because a
message of unbounded length has no place in a real-time buffer.

### Parameter event — 16 bytes

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `u32` | `frame` |
| 4 | `u32` | `index` |
| 8 | `f64` | `value` |

Sample-accurate automation, delivered with the block it belongs to. A plugin
may also be told about parameters outside the audio path through
`rackforge_set_parameter`.

### Transfer buffer

A scratch area for everything that is not real time: preset catalogues, saved
state, resource chunks, program documents. The host writes into it before a
call that consumes bytes, and reads out of it after a call that produces them.
It is only valid during the call that names it.

## Required exports

Twenty-three functions and the memory. The signatures are given in WebAssembly
types; `i32` is a signed 32-bit integer.

### Instance

```wat
(func (export "rackforge_initialize") (result i32))
(func (export "rackforge_prepare")
      (param $sample_rate f64) (param $maximum_frames i32)
      (param $input_channels i32) (param $output_channels i32) (result i32))
(func (export "rackforge_reset") (result i32))
```

`initialize` runs once and sets up whatever the buffers need. `prepare` gives
the audio format and may be called again when the format changes; it must be
called before `process`. `reset` clears sounding voices and any tail, without
disturbing parameters.

### Parameters

```wat
(func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32))
(func (export "rackforge_get_parameter") (param $index i32) (result f64))
```

`set_parameter` returns `-2` for an index it does not know. `get_parameter`
returns the value; it has no error channel, so return a sane default for an
unknown index.

### Audio

```wat
(func (export "rackforge_process")
      (param $frames i32) (param $input_channels i32) (param $output_channels i32)
      (param $midi_event_count i32) (param $parameter_event_count i32) (result i32))
```

Read `midi_event_count` events from the MIDI buffer and
`parameter_event_count` from the parameter buffer, consume
`frames * input_channels` samples from the input buffer, and write exactly
`frames * output_channels` samples to the output buffer. Return `0`.

This function runs on the audio thread. It must not block, and it must not
depend on anything it cannot compute from what it was handed.

### State

```wat
(func (export "rackforge_save_state") (result i32))
(func (export "rackforge_load_state") (param $length i32) (result i32))
```

`save_state` writes its bytes into the transfer buffer and returns **the byte
count**. `load_state` reads `length` bytes that the host has already placed
there. Return `-3` from both if the plugin has no state worth keeping.

### Presets

```wat
(func (export "rackforge_load_preset") (param $length i32) (result i32))
```

The preset id, `length` bytes of UTF-8, is in the transfer buffer.

### Resources

```wat
(func (export "rackforge_resource_begin") (param $id_length i32) (param $total_bytes i64) (result i32))
(func (export "rackforge_resource_write") (param $offset i64) (param $length i32) (result i32))
(func (export "rackforge_resource_end") (result i32))
```

How a sound bank or ROM larger than the transfer buffer arrives: `begin` names
it (the id is in the transfer buffer) and declares the total size, `write`
delivers one chunk at a time at a byte offset, `end` closes it. Return `-3`
from all three if the plugin loads no external files.

## Optional extensions

The host looks these up and does without them if they are absent. Nothing here
is needed for a plugin that makes sound.

| Export | What it adds |
| --- | --- |
| `rackforge_preset_catalog` | Publishes presets to the host, rather than only accepting them. Writes into the transfer buffer, returns the byte count. |
| `rackforge_process_v2` | `process` plus a sixth parameter, `midi2_event_count`. Requires the three MIDI 2.0 exports below. |
| `rackforge_midi2_ptr`, `rackforge_capacity_midi2_events`, `rackforge_midi2_families` | The MIDI 2.0 buffer, and which message families the plugin wants at 2.0 width. |
| `rackforge_exchange_input_ptr` | A separate input area for the program-editing calls. |
| `rackforge_program_*` | Editing individual programs from the host's interface. See [WEB_PLUGIN_API.md](WEB_PLUGIN_API.md). |
| `rackforge_parallel_*` | Splitting one block across the host's audio workers. The complete contract is in [PARALLEL_RENDER.md](PARALLEL_RENDER.md). |

### MIDI 2.0 event — 16 bytes

Two little-endian `u64`s. The first:

| Bits | Field |
| --- | --- |
| 0–31 | `frame` |
| 32–39 | `kind` |
| 40–47 | `channel` |
| 48–55 | `index` — note, controller or program number |
| 56–63 | `flags` |

The second is `value` in bits 0–31 and `extra` in bits 32–63. `kind` is 1
note-off, 2 note-on, 3 poly pressure, 4 control change, 5 program change, 6
channel pressure, 7 pitch bend. Note velocities occupy the low 16 bits of
`value`; controllers, pressure and bend use all 32, with pitch bend unsigned
and centred at `1 << 31`. Bit 0 of `flags` means the value was widened from a
7-bit source, so a plugin can recover the original byte exactly and behave as
it always did.

Declare the families you want in `rackforge_midi2_families`; everything else
keeps arriving as MIDI 1.0. A plugin that exports none of this receives
everything at 1.0 width, which is why 16-bit velocity is an extension and not
a migration.

## Call order

```text
rackforge_abi_version
rackforge_initialize
  the buffer addresses and capacities, once
rackforge_prepare
  ├─ rackforge_load_state / rackforge_load_preset / resource_begin…write…end
  ├─ rackforge_set_parameter, at any time
  ├─ rackforge_process, once per audio block
  ├─ rackforge_reset, when sound must stop
  └─ rackforge_save_state, at any time
rackforge_prepare, again if the audio format changes
```

Everything before `prepare` returns `-3` if it needs a prepared instance.

## A complete plugin

This is a working gain, in WebAssembly text, with nothing but the ABI. It is
close to what the host's own loader tests use, and it is the shortest honest
answer to "how much do I have to write".

```wat
(module
  (memory (export "memory") 1)
  (global $gain (mut f32) (f32.const 1))

  (func (export "rackforge_abi_version") (result i32) i32.const 0x00010001)

  ;; Buffers: input at 0, output at 1024, MIDI at 4096, parameters at 5120,
  ;; transfer at 8192 — none of them overlapping, all inside one 64 KiB page.
  (func (export "rackforge_input_ptr") (result i32) i32.const 0)
  (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
  (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
  (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
  (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
  (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
  (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
  (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
  (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
  (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)

  (func (export "rackforge_initialize") (result i32) i32.const 0)
  (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
  (func (export "rackforge_reset") (result i32) i32.const 0)

  (func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32)
    local.get $value f32.demote_f64 global.set $gain
    i32.const 0)
  (func (export "rackforge_get_parameter") (param $index i32) (result f64)
    global.get $gain f64.promote_f32)

  ;; Declined, and declining is a supported answer.
  (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
  (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
  (func (export "rackforge_resource_end") (result i32) i32.const -3)
  (func (export "rackforge_load_preset") (param i32) (result i32) i32.const -3)
  (func (export "rackforge_save_state") (result i32) i32.const -3)
  (func (export "rackforge_load_state") (param i32) (result i32) i32.const -3)

  (func (export "rackforge_process")
        (param $frames i32) (param $input_channels i32) (param $output_channels i32)
        (param $midi i32) (param $parameters i32) (result i32)
    (local $i i32) (local $count i32)
    ;; The newest parameter event wins; its f64 sits 8 bytes into the record.
    local.get $parameters i32.const 0 i32.gt_s
    if
      i32.const 5128 f64.load f32.demote_f64 global.set $gain
    end
    local.get $frames local.get $output_channels i32.mul local.set $count
    (block $done
      (loop $copy
        local.get $i local.get $count i32.ge_s br_if $done
        local.get $i i32.const 4 i32.mul i32.const 1024 i32.add
        local.get $i i32.const 4 i32.mul f32.load
        global.get $gain f32.mul
        f32.store
        local.get $i i32.const 1 i32.add local.set $i
        br $copy))
    i32.const 0))
```

## The same thing in C

[`plugins/gain-c/gain.c`](../plugins/gain-c/gain.c) is this contract
implemented in freestanding C -- no SDK, no libc, one file, about 2 KB of
WebAssembly:

```bash
clang --target=wasm32 -nostdlib -O2       -Wl,--no-entry -Wl,--export-memory       -o gain.wasm gain.c
```

It is compiled with `-Werror`, loaded through the real portable loader and
played by `crates/rackforge-plugin-runtime/tests/c_reference_plugin.rs`, which
CI cannot skip. If this document and that plugin ever disagree, the test says
so before a plugin author does.

## Rules a host will hold you to

- **Never import anything.** The module is refused before it runs.
- **Publish stable buffer addresses and capacities.** They are read once.
- **Do not let buffers overlap**, and keep them inside linear memory.
- **Write exactly `frames * output_channels` samples**, every block.
- **Do not block in `process`.** It runs on the audio thread, under a deadline.
- **Decline with `-3`** rather than pretending; the host handles it.
- **Return the byte count** from the functions that produce data, and never
  more than the capacity you published.
