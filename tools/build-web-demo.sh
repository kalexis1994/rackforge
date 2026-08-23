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

# Windows ships the interpreter as `python`; most Linux distributions ship it
# only as `python3`. Run candidates before trusting the Microsoft Store shim.
python=
for candidate in python3 python; do
  if "$candidate" -c "" >/dev/null 2>&1; then python=$candidate; break; fi
done
[ -n "$python" ] || { echo "no working python interpreter on PATH" >&2; exit 1; }

echo "Building the browser host"
cargo build --release --target wasm32-wasip1 -p rackforge-browser --manifest-path "$root/Cargo.toml"

echo "Building the bundled instrument"
cargo build --release --target wasm32-unknown-unknown \
  -p rackforge-concert-grand --manifest-path "$root/Cargo.toml"

rm -rf "$public"
mkdir -p "$storage/plugins"

cp "$root/target/wasm32-wasip1/release/rackforge_browser.wasm" "$public/rackforge-browser.wasm"

# The Concert Grand is the instrument RackForge ships; the demo synth was a
# scaffold from before it existed and only muddied what the demo is for.
piano="$storage/plugins/concert-grand"
cp -r "$root/plugins/concert-grand/package" "$piano"
cp "$root/target/wasm32-unknown-unknown/release/rackforge_concert_grand.wasm" "$piano/component.wasm"

"$python" "$root/tools/fetch-official-plugins.py" \
  --output-directory "$root/dist/bundled-plugins/official" \
  --extract-directory "$storage/plugins"

# The host reads its storage through WASI, which has no way to list what the
# page has not fetched yet. The manifest is that list.
"$python" - "$storage" > "$public/storage.json" <<'PY'
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

# The page itself is built separately, and the flag must travel WITH the
# command -- never through web/.env.local. A stray .env.local carrying
# VITE_RACKFORGE_BROWSER_HOST=1 once turned every plain npm build into a
# browser-host build; the desktop app embedded one, its WebView booted a
# complete second instrument (own AudioContext, own WebMIDI permission
# prompt), and every note played two pianos at once.
echo "Now build the page with:  VITE_RACKFORGE_BROWSER_HOST=1 npm run build"
