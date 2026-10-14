#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/install-env.sh"
rackforge_resolve_install_environment

root="$RACKFORGE_ROOT_RESOLVED"
source_root="${RACKFORGE_SOURCE:-$root/current}"
web_source_root="${RACKFORGE_WEB_SOURCE:-$root/current/web}"

test -x "$source_root/target/release/rackforge-core"
test -x "$source_root/target/release/rackforge-web"
test -x "$source_root/target/release/rackforge-store"
test -x "$source_root/target/release/rackforge-platform-host"
test -x "$source_root/target/release/rackforge-controller-host"
test -f "$source_root/platforms/raspberry-pi/config/rackforge.toml"
test -f "$web_source_root/dist/index.html"

# Every directory the systemd units name in their mount namespace has to
# exist before they start. A unit naming a missing path does not warn: it
# fails at step NAMESPACE and takes the service down with it, which is how
# a clean install of the appliance came up with no web interface while
# every upgrade over an older root worked. plugins/ is empty on a fresh
# install and was the one nothing else happened to create.
install -d \
  "$root/bin" \
  "$root/config" \
  "$root/plugins" \
  "$root/plugin-store" \
  "$root/state" \
  "$root/logs"
for binary in \
  rackforge-core \
  rackforge-web \
  rackforge-store \
  rackforge-platform-host \
  rackforge-controller-host
do
  install -m 0755 \
    "$source_root/target/release/$binary" \
    "$root/bin/$binary.new"
  mv "$root/bin/$binary.new" "$root/bin/$binary"
done

# Every package the release carries, Concert Grand included, not one name
# written here: the bundle decides which ones travel, so a new one needs no
# edit. The store's official install skips a plugin the player uninstalled,
# enables a newcomer, leaves one it already knows as the player set it, and
# prunes versions older than the two newest.
bundled_plugins=none
shopt -s nullglob
for official_plugin in "$source_root/bundled-plugins"/*.rfplugin; do
  install_output="$("$root/bin/rackforge-store" install-local \
    "$official_plugin" \
    "$root/plugin-store" --official)"
  printf '%s\n' "$install_output"
  # Read the id the store just reported. No pipeline here on purpose:
  # under `set -o pipefail` a `... | head -1` ends in SIGPIPE and takes
  # the whole installer down after the first plugin.
  if [[ "$install_output" =~ PLUGIN_REMOVED_BY_PLAYER[[:space:]]+id= ]]; then
    continue
  fi
  official_plugin_id=""
  if [[ "$install_output" =~ PLUGIN_INSTALLED[[:space:]]+id=([^[:space:]]+) ]]; then
    official_plugin_id="${BASH_REMATCH[1]}"
  fi
  if [[ -z "$official_plugin_id" ]]; then
    printf 'could not read the plugin id installed from %s\n' "$official_plugin" >&2
    exit 1
  fi
  if [[ "$bundled_plugins" == none ]]; then
    bundled_plugins="$official_plugin_id"
  else
    bundled_plugins="$bundled_plugins,$official_plugin_id"
  fi
done
shopt -u nullglob

controller_package="$source_root/controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"
if [[ -d "$controller_package" ]]; then
  install -d "$root/controllers"
  "$root/bin/rackforge-controller-host" verify "$controller_package"
  "$root/bin/rackforge-controller-host" install \
    "$controller_package" \
    --root "$root/controllers" \
    --trust official
fi

# The controllers RackForge describes from their makers' documentation. The
# controller host's service sees its store read-only, so they are installed
# here, with every release.
install -d "$root/controllers"
"$root/bin/rackforge-controller-host" install-catalog --root "$root/controllers"

if [ ! -f "$root/config/rackforge.toml" ]; then
  install -m 0644 \
    "$source_root/platforms/raspberry-pi/config/rackforge.toml" \
    "$root/config/rackforge.toml"
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

printf 'RUNTIME_INSTALLED root=%s bundled_plugins=%s web=%s\n' \
  "$root" "$bundled_plugins" "$root/web"
