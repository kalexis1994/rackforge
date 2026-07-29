#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current/runtime}"
plugin_root="$root/plugins/roland-scva"

test -x "$source_root/target/release/rackforge-core"
test -f "$source_root/target/release/librackforge_roland_scva.so"
test -f "$source_root/plugins/roland-scva/package/rackforge-plugin.toml"

install -d "$root/bin" "$plugin_root/lib" "$root/state" "$root/logs"
install -m 0755 \
  "$source_root/target/release/rackforge-core" \
  "$root/bin/rackforge-core.new"
mv "$root/bin/rackforge-core.new" "$root/bin/rackforge-core"

install -m 0644 \
  "$source_root/plugins/roland-scva/package/rackforge-plugin.toml" \
  "$plugin_root/rackforge-plugin.toml.new"
mv "$plugin_root/rackforge-plugin.toml.new" "$plugin_root/rackforge-plugin.toml"

install -m 0755 \
  "$source_root/target/release/librackforge_roland_scva.so" \
  "$plugin_root/lib/librackforge_roland_scva.so.new"
mv \
  "$plugin_root/lib/librackforge_roland_scva.so.new" \
  "$plugin_root/lib/librackforge_roland_scva.so"

printf 'RUNTIME_INSTALLED root=%s plugin=%s\n' "$root" "$plugin_root"
