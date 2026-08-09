#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current}"
web_source_root="${RACKFORGE_WEB_SOURCE:-$root/current/web}"

test -x "$source_root/target/release/rackforge-core"
test -x "$source_root/target/release/rackforge-web"
test -f "$source_root/config/rackforge.toml"
test -f "$source_root/config/repositories.toml"
test -f "$web_source_root/dist/index.html"

install -d \
  "$root/bin" \
  "$root/config" \
  "$root/plugin-store" \
  "$root/state" \
  "$root/logs"
install -m 0755 \
  "$source_root/target/release/rackforge-core" \
  "$root/bin/rackforge-core.new"
mv "$root/bin/rackforge-core.new" "$root/bin/rackforge-core"

install -m 0755 \
  "$source_root/target/release/rackforge-web" \
  "$root/bin/rackforge-web.new"
mv "$root/bin/rackforge-web.new" "$root/bin/rackforge-web"

if [ ! -f "$root/config/rackforge.toml" ]; then
  install -m 0644 \
    "$source_root/config/rackforge.toml" \
    "$root/config/rackforge.toml"
fi

if [ ! -f "$root/config/repositories.toml" ]; then
  install -m 0644 \
    "$source_root/config/repositories.toml" \
    "$root/config/repositories.toml"
fi

web_stage="$(mktemp -d "$root/.web-stage.XXXXXX")"
web_previous="$root/.web-previous"
trap 'rm -rf "$web_stage"' EXIT
cp -R "$web_source_root/dist/." "$web_stage/"
rm -rf "$web_previous"
if [ -d "$root/web" ]; then
  mv "$root/web" "$web_previous"
fi
mv "$web_stage" "$root/web"
rm -rf "$web_previous"
trap - EXIT

printf 'RUNTIME_INSTALLED root=%s bundled_plugins=none web=%s\n' \
  "$root" "$root/web"
