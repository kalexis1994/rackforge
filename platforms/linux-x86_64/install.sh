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
source "$script_dir/scripts/lib/render-audio-seed.sh"
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

# Every package the release carries, Concert Grand included, goes through
# the store's official install: a plugin the player uninstalled stays out, a
# newcomer is enabled, one the store already knows keeps whatever the player
# chose, and versions older than the two newest are pruned. See the Raspberry
# Pi installer for the same rule.
#
# Concert Grand was once offered only on a first install, behind this marker,
# so a player could remove it for good before the store recorded uninstalls.
# A store that went through that offer and no longer holds it lost it to such
# an uninstall: leave it out, as a recorded uninstall would. The marker is
# still written for an older installer run over this one.
concert_grand_id=org.rackforge.concert-grand
default_marker="$root/state/bundled-default-initialized"
skip_concert_grand=0
if [[ -f "$default_marker" && ! -d "$root/plugin-store/packages/$concert_grand_id" ]]; then
  skip_concert_grand=1
fi
shopt -s nullglob
for official_plugin in "$source_root/bundled-plugins"/*.rfplugin; do
  if [[ "$(basename "$official_plugin")" == "RF-Concert-Grand.rfplugin" \
    && "$skip_concert_grand" -eq 1 ]]; then
    continue
  fi
  install_output="$("$root/bin/rackforge-store" install-local \
    "$official_plugin" "$root/plugin-store" --official)"
  printf '%s\n' "$install_output"
  # Read what the store just reported. No pipeline here on purpose:
  # under `set -o pipefail` a `... | head -1` ends in SIGPIPE and takes
  # the whole installer down after the first plugin.
  if [[ ! "$install_output" =~ PLUGIN_(INSTALLED|REMOVED_BY_PLAYER)[[:space:]]+id=[^[:space:]]+ ]]; then
    printf 'could not read the plugin id installed from %s\n' "$official_plugin" >&2
    exit 1
  fi
  if [[ "$(basename "$official_plugin")" == "RF-Concert-Grand.rfplugin" ]]; then
    printf '1\n' >"$default_marker"
  fi
done
shopt -u nullglob

# Rendered, not copied: the seed names @RACKFORGE_ROOT@ and the bundled
# instrument, because Core refuses a relative `package` or `data_root`. After
# the plugins, as the Pi's installers order it: the seed points at the
# installed Concert Grand, and rendered before it was installed it found no
# instrument and was skipped, so a fresh install had no audio configuration
# until the installer ran a second time.
if [[ ! -f "$root/config/audio.toml.example" && -f "$source_root/config/audio.toml" ]]; then
  rackforge_render_audio_seed \
    "$source_root/config/audio.toml" \
    "$root/config/audio.toml.example"
fi

controller_package="$source_root/controller-packages/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"
if [[ -d "$controller_package" ]]; then
  "$root/bin/rackforge-controller-host" verify "$controller_package"
  "$root/bin/rackforge-controller-host" install \
    "$controller_package" --root "$root/controllers" --trust official
fi

# The controllers RackForge describes from their makers' documentation; the
# controller host's service sees its store read-only, so they go in here.
install -d "$root/controllers"
"$root/bin/rackforge-controller-host" install-catalog --root "$root/controllers"

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

address="$(rackforge_lan_address)"
printf 'RACKFORGE_LINUX_INSTALLED root=%s web=http://%s:8787\n' \
  "$root" "${address:-127.0.0.1}"
printf 'Log out and back in once if this user was newly added to the audio group.\n'
