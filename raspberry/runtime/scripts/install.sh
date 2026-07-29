#!/usr/bin/env bash
set -euo pipefail

root="${ARTUPY_ROOT:-/home/kalex/artupy}"
source_root="${ARTUPY_SOURCE:-$root/current/runtime}"
plugin_root="$root/plugins/roland-scva"

test -x "$source_root/target/release/artupy-core"
test -f "$source_root/target/release/libartupy_roland_scva.so"
test -f "$source_root/plugins/roland-scva/package/artupy-plugin.toml"

install -d "$root/bin" "$plugin_root/lib" "$root/state" "$root/logs"
install -m 0755 \
  "$source_root/target/release/artupy-core" \
  "$root/bin/artupy-core.new"
mv "$root/bin/artupy-core.new" "$root/bin/artupy-core"

install -m 0644 \
  "$source_root/plugins/roland-scva/package/artupy-plugin.toml" \
  "$plugin_root/artupy-plugin.toml.new"
mv "$plugin_root/artupy-plugin.toml.new" "$plugin_root/artupy-plugin.toml"

install -m 0755 \
  "$source_root/target/release/libartupy_roland_scva.so" \
  "$plugin_root/lib/libartupy_roland_scva.so.new"
mv \
  "$plugin_root/lib/libartupy_roland_scva.so.new" \
  "$plugin_root/lib/libartupy_roland_scva.so"

printf 'RUNTIME_INSTALLED root=%s plugin=%s\n' "$root" "$plugin_root"
