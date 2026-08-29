#!/usr/bin/env bash
set -euo pipefail

case "$(uname -m)" in
  x86_64|amd64) ;;
  *)
    printf 'RackForge Linux x86-64 requires an x86-64 userspace, got %s.\n' "$(uname -m)" >&2
    exit 2
    ;;
esac

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/scripts/lib/install-env.sh"
rackforge_resolve_install_environment

root="$RACKFORGE_ROOT_RESOLVED"
source_root="${RACKFORGE_SOURCE:-$(cd "$script_dir/../.." && pwd)}"
service_user="$RACKFORGE_USER_RESOLVED"

for binary in \
  rackforge-core rackforge-web rackforge-store rackforge-platform-host rackforge-controller-host
do
  test -x "$source_root/target/release/$binary"
done
test -f "$source_root/web/dist/index.html"
test -f "$script_dir/config/rackforge.toml"

sudo -v
sudo groupadd --system --force rackforge
sudo groupadd --system --force audio
sudo usermod -a -G audio,rackforge "$service_user"

install -d \
  "$root/bin" "$root/config" "$root/data" "$root/state" "$root/logs" \
  "$root/plugin-store" "$root/plugins" "$root/controllers"
for binary in \
  rackforge-core rackforge-web rackforge-store rackforge-platform-host rackforge-controller-host
do
  install -m 0755 "$source_root/target/release/$binary" "$root/bin/$binary.new"
  mv "$root/bin/$binary.new" "$root/bin/$binary"
done

if [[ ! -f "$root/config/rackforge.toml" ]]; then
  install -m 0644 "$script_dir/config/rackforge.toml" "$root/config/rackforge.toml"
fi
if [[ ! -f "$root/config/audio.toml.example" && -f "$source_root/config/audio.toml" ]]; then
  install -m 0644 "$source_root/config/audio.toml" "$root/config/audio.toml.example"
fi

web_stage="$(mktemp -d "$root/.web-stage.XXXXXX")"
trap 'rm -rf "$web_stage"' EXIT
cp -R "$source_root/web/dist/." "$web_stage/"
rm -rf "$root/web.previous"
if [[ -d "$root/web" ]]; then
  mv "$root/web" "$root/web.previous"
fi
mv "$web_stage" "$root/web"
rm -rf "$root/web.previous"
trap - EXIT

concert_grand="$source_root/bundled-plugins/RackForge-Concert-Grand.rfplugin"
default_marker="$root/state/bundled-default-initialized"
shopt -s nullglob
installed_plugins=("$root/plugin-store/packages"/*)
shopt -u nullglob
if [[ ! -f "$default_marker" ]]; then
  if [[ -f "$concert_grand" && ${#installed_plugins[@]} -eq 0 ]]; then
    "$root/bin/rackforge-store" install-local "$concert_grand" "$root/plugin-store"
    "$root/bin/rackforge-store" enable org.rackforge.concert-grand "$root/plugin-store"
  fi
  if [[ -f "$concert_grand" || ${#installed_plugins[@]} -gt 0 ]]; then
    printf '1\n' >"$default_marker"
  fi
fi

# Every official instrument the release carries; see the Raspberry Pi
# installer for the same rule. A newcomer is enabled, a plugin the store
# already knows keeps whatever the player chose.
known_ids=" $(ls "$root/plugin-store/packages" 2>/dev/null | tr '\n' ' ') "
shopt -s nullglob
for official_plugin in "$source_root/bundled-plugins"/*.rfplugin; do
  [[ "$(basename "$official_plugin")" == "RackForge-Concert-Grand.rfplugin" ]] && continue
  install_output="$("$root/bin/rackforge-store" install-local \
    "$official_plugin" "$root/plugin-store")"
  printf '%s\n' "$install_output"
  official_plugin_id="$(printf '%s\n' "$install_output" |
    sed -n 's/.*PLUGIN_INSTALLED id=\([^ ]*\).*/\1/p' | head -1)"
  [[ -n "$official_plugin_id" ]] || {
    printf 'could not read the plugin id installed from %s\n' "$official_plugin" >&2
    exit 1
  }
  if [[ "$known_ids" != *" $official_plugin_id "* ]]; then
    "$root/bin/rackforge-store" enable "$official_plugin_id" "$root/plugin-store"
  fi
done
shopt -u nullglob

controller_package="$source_root/controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"
if [[ -d "$controller_package" ]]; then
  "$root/bin/rackforge-controller-host" verify "$controller_package"
  "$root/bin/rackforge-controller-host" install \
    "$controller_package" --root "$root/controllers" --trust official
fi

for unit in rackforge-platform-host rackforge-controller-host rackforge-web rackforge-audio
do
  rackforge_render_systemd_unit \
    "$script_dir/systemd/$unit.service" "/etc/systemd/system/$unit.service"
done
rackforge_render_systemd_unit \
  "$script_dir/systemd/rackforge-audio.path" "/etc/systemd/system/rackforge-audio.path"

sudo systemctl daemon-reload
sudo systemctl enable \
  rackforge-platform-host.service rackforge-controller-host.service \
  rackforge-web.service rackforge-audio.service rackforge-audio.path
sudo systemctl restart \
  rackforge-platform-host.service rackforge-controller-host.service \
  rackforge-web.service rackforge-audio.path
if [[ -f "$root/config/audio.toml" ]]; then
  sudo systemctl restart rackforge-audio.service
else
  sudo systemctl stop rackforge-audio.service
fi

address="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
printf 'RACKFORGE_LINUX_INSTALLED root=%s web=http://%s:8787\n' \
  "$root" "${address:-127.0.0.1}"
printf 'Log out and back in once if this user was newly added to the audio group.\n'
