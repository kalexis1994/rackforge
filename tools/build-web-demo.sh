#!/usr/bin/env bash
# Builds the static RackForge demo: the browser host, the instrument it plays,
# and the storage image it boots against.
#
# The result is a plain directory of files, so it can be served by anything —
# GitHub Pages, a local static server, or `vite preview`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
public="$root/web/public/demo"
storage="$public/rackforge"

echo "Building the browser host"
cargo build --release --target wasm32-wasip1 -p rackforge-browser --manifest-path "$root/Cargo.toml"

echo "Building the demo instrument"
cargo build --release --target wasm32-unknown-unknown -p rackforge-demo-synth --manifest-path "$root/Cargo.toml"

rm -rf "$public"
mkdir -p "$storage/plugins"

cp "$root/target/wasm32-wasip1/release/rackforge_browser.wasm" "$public/rackforge-browser.wasm"

plugin="$storage/plugins/demo-synth"
cp -r "$root/plugins/demo-synth/package" "$plugin"
cp "$root/target/wasm32-unknown-unknown/release/rackforge_demo_synth.wasm" "$plugin/component.wasm"

# The host reads its storage through WASI, which has no way to list what the
# page has not fetched yet. The manifest is that list.
python3 - "$storage" > "$public/storage.json" <<'PY'
import json
import os
import sys

root = sys.argv[1]
files = []
for directory, _, names in os.walk(root):
    for name in sorted(names):
        path = os.path.join(directory, name)
        files.append(os.path.relpath(path, root).replace(os.sep, "/"))
print(json.dumps({"files": sorted(files)}, indent=2))
PY

echo "Demo assets written to $public"
