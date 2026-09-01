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

bundled_plugin="$source_root/bundled-plugins/RF-Concert-Grand.rfplugin"
bundled_default_marker="$root/state/bundled-default-initialized"
shopt -s nullglob
installed_plugins=("$root/plugin-store/packages"/*)
shopt -u nullglob
bundled_plugins=none
if [[ ! -f "$bundled_default_marker" ]]; then
  if [[ -f "$bundled_plugin" && ${#installed_plugins[@]} -eq 0 ]]; then
    "$root/bin/rackforge-store" install-local \
      "$bundled_plugin" \
      "$root/plugin-store"
    "$root/bin/rackforge-store" enable \
      org.rackforge.concert-grand \
      "$root/plugin-store"
    bundled_plugins=org.rackforge.concert-grand
  fi
  if [[ -f "$bundled_plugin" || ${#installed_plugins[@]} -gt 0 ]]; then
    printf '1\n' > "$bundled_default_marker"
  fi
fi

# Every official instrument the release carries, not one name written here:
# the bundle decides which ones travel, so a new one needs no edit. A plugin
# the store already knows is installed but left as the player set it — only a
# newcomer is enabled on their behalf.
known_ids=" $(ls "$root/plugin-store/packages" 2>/dev/null | tr '\n' ' ') "
shopt -s nullglob
for official_plugin in "$source_root/bundled-plugins"/*.rfplugin; do
  [[ "$(basename "$official_plugin")" == "RF-Concert-Grand.rfplugin" ]] && continue
  install_output="$("$root/bin/rackforge-store" install-local \
    "$official_plugin" \
    "$root/plugin-store" --replace)"
  printf '%s\n' "$install_output"
  # Read the id the store just reported. No pipeline here on purpose:
  # under `set -o pipefail` a `... | head -1` ends in SIGPIPE and takes
  # the whole installer down after the first plugin.
  official_plugin_id=""
  if [[ "$install_output" =~ PLUGIN_INSTALLED[[:space:]]+id=([^[:space:]]+) ]]; then
    official_plugin_id="${BASH_REMATCH[1]}"
  fi
  if [[ -z "$official_plugin_id" ]]; then
    printf 'could not read the plugin id installed from %s\n' "$official_plugin" >&2
    exit 1
  fi
  if [[ "$known_ids" != *" $official_plugin_id "* ]]; then
    "$root/bin/rackforge-store" enable \
      "$official_plugin_id" \
      "$root/plugin-store"
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
