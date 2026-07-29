#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current/runtime}"
roland_plugin_root="$root/plugins/roland-scva"
rf_dls_plugin_root="$root/plugins/rf-dls"

test -x "$source_root/target/release/rackforge-core"
test -f "$source_root/target/release/librackforge_roland_scva.so"
test -f "$source_root/target/release/librackforge_rf_dls.so"
test -f "$source_root/plugins/roland-scva/package/rackforge-plugin.toml"
test -f "$source_root/plugins/rf-dls/package/rackforge-plugin.toml"

install -d \
  "$root/bin" \
  "$roland_plugin_root/lib" \
  "$rf_dls_plugin_root/lib" \
  "$root/state" \
  "$root/logs"
install -m 0755 \
  "$source_root/target/release/rackforge-core" \
  "$root/bin/rackforge-core.new"
mv "$root/bin/rackforge-core.new" "$root/bin/rackforge-core"

install -m 0644 \
  "$source_root/plugins/roland-scva/package/rackforge-plugin.toml" \
  "$roland_plugin_root/rackforge-plugin.toml.new"
mv \
  "$roland_plugin_root/rackforge-plugin.toml.new" \
  "$roland_plugin_root/rackforge-plugin.toml"

install -m 0755 \
  "$source_root/target/release/librackforge_roland_scva.so" \
  "$roland_plugin_root/lib/librackforge_roland_scva.so.new"
mv \
  "$roland_plugin_root/lib/librackforge_roland_scva.so.new" \
  "$roland_plugin_root/lib/librackforge_roland_scva.so"

install -m 0644 \
  "$source_root/plugins/rf-dls/package/rackforge-plugin.toml" \
  "$rf_dls_plugin_root/rackforge-plugin.toml.new"
mv \
  "$rf_dls_plugin_root/rackforge-plugin.toml.new" \
  "$rf_dls_plugin_root/rackforge-plugin.toml"

install -m 0755 \
  "$source_root/target/release/librackforge_rf_dls.so" \
  "$rf_dls_plugin_root/lib/librackforge_rf_dls.so.new"
mv \
  "$rf_dls_plugin_root/lib/librackforge_rf_dls.so.new" \
  "$rf_dls_plugin_root/lib/librackforge_rf_dls.so"

printf 'RUNTIME_INSTALLED root=%s plugins=%s,%s\n' \
  "$root" "$roland_plugin_root" "$rf_dls_plugin_root"
