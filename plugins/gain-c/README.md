# gain-c — a RackForge plugin in C

A complete `wasm-v1` instrument in one freestanding C file, with no SDK, no
libc and no generated code. It exists to keep
[docs/PLUGIN_ABI.md](../../docs/PLUGIN_ABI.md) honest: the ABI is a plain
WebAssembly contract, and this is what implementing it directly looks like.

It is a gain. Audio in, audio out, scaled by a parameter the host can set — or
that MIDI controller 7 moves, which is the volume slider a keyboard already
sends.

## Build

Any clang with the `wasm32` target:

```bash
clang --target=wasm32 -nostdlib -O2 \
      -Wl,--no-entry -Wl,--export-memory \
      -o gain.wasm gain.c
```

That is the whole toolchain. `-nostdlib` is not an optimisation — it is what
keeps the module free of imports, and RackForge refuses a plugin that imports
anything at all.

The result is about 2 KB.

## It is tested, not just published

`crates/rackforge-plugin-runtime/tests/c_reference_plugin.rs` compiles this
file with `-Werror`, loads it through the same portable loader a real
installation uses, and plays it: once through the host's parameter path, and
once through the MIDI buffer, where the plugin decodes the packed 64-bit event
itself.

The test skips where no clang emits `wasm32`, so working on the rest of
RackForge does not require one. It cannot skip in CI, which sets
`RACKFORGE_REQUIRE_WASM_CC=1` and turns the skip into a failure.

To run it against a particular compiler:

```bash
RACKFORGE_WASM_CC=/path/to/clang cargo test -p rackforge-plugin-runtime --test c_reference_plugin
```

## What it does not do

It is a reference for the ABI, not a template for an instrument. It declines
saved state, presets and external resources by returning `-3`, which is the
supported answer for anything a plugin does not implement. It has no `.rfplugin`
package around it — for the manifest, branding and packaging that make a plugin
installable, see [docs/PLUGIN_DEVELOPMENT.md](../../docs/PLUGIN_DEVELOPMENT.md).
