# rackforge-plugin-sdk

Write a [RackForge](https://github.com/kalexis1994/rackforge) instrument in
Rust: implement one trait, export it with one macro, and the result is a
portable `.wasm` that plays on Windows, Linux, Android, a Raspberry Pi and in
a browser tab — the same bytes on all five.

```rust
use rackforge_plugin_sdk::{export_processor, MidiEvent, ParameterEvent, Processor};

#[derive(Default)]
struct Gain {
    gain: f32,
}

impl Processor for Gain {
    fn prepare(&mut self, _sample_rate: f64, _frames: u32, _inputs: u32, _outputs: u32) -> bool {
        self.gain = 1.0;
        true
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        if index != 0 {
            return false;
        }
        self.gain = value as f32;
        true
    }

    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        _midi: &[MidiEvent],
        parameters: &[ParameterEvent],
        _frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) {
        // Sample-accurate automation arrives with the block it belongs to.
        for event in parameters {
            if event.index == 0 {
                self.gain = event.value as f32;
            }
        }
        for (out, sample) in output.iter_mut().zip(input) {
            *out = sample * self.gain;
        }
    }
}

// The buffer capacities the host will read once and hold you to: the largest
// block, channel count and event count this plugin can survive.
export_processor!(
    Gain,
    max_frames = 4096,
    max_input_channels = 2,
    max_output_channels = 2,
    max_midi_events = 256,
    max_parameter_events = 256,
    max_transfer_bytes = 4096
);
```

```bash
cargo build --release --target wasm32-unknown-unknown
```

## This is one implementation, not the contract

RackForge plugins speak a plain WebAssembly ABI: exported functions taking
integers and floats, and buffers the host reads and writes inside the module's
own linear memory. Nothing about it is specific to Rust, and the host cannot
tell what a module was written in.

This crate is the comfortable way to reach that ABI *if you are writing Rust*.
It owns the raw exports, the linear-memory buffers and the packed event
decoding so your code sees slices and a trait.

If you would rather implement the ABI directly — in C, Zig, TinyGo,
AssemblyScript, or WebAssembly text — the contract is
[docs/PLUGIN_ABI.md](https://github.com/kalexis1994/rackforge/blob/main/docs/PLUGIN_ABI.md),
and [`plugins/gain-c/gain.c`](https://github.com/kalexis1994/rackforge/blob/main/plugins/gain-c/gain.c)
is a complete instrument in freestanding C, about 2 KB compiled.

## What the SDK gives you

- **`Processor`** — `prepare`, `process`, `reset`, parameters, and optional
  state, presets and external resources. Anything you do not implement is
  declined for you.
- **`export_processor!`** — generates every export the host looks up, with the
  argument checking and buffer bookkeeping.
- **MIDI at both widths.** `MidiEvent` for MIDI 1.0, and `MidiEvent2` for
  16-bit velocities and 32-bit controllers where the host can supply them.
- **`export_parallel_processor!`** — for instruments whose block splits into
  independent units, so RackForge can spread them across its audio workers
  while single-core hosts run the identical component sequentially.

`#![no_std]`, no allocator, no dependencies.

## Packaging

A `.wasm` is not yet an installable instrument: it needs a manifest, branding
and a package around it.
[docs/PLUGIN_DEVELOPMENT.md](https://github.com/kalexis1994/rackforge/blob/main/docs/PLUGIN_DEVELOPMENT.md)
covers the `.rfplugin` package and how RackForge validates it before it runs.

## Versioning

The crate version tracks the RackForge release it was cut from. The plugin API
is versioned separately in the package manifest: a host keeps loading packages
that declare an older minor, and a package declares the first host minor
providing everything it uses.
