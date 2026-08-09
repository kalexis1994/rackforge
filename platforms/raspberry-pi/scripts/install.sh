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
test -f "$source_root/config/repositories.toml"
test -f "$web_source_root/dist/index.html"

install -d \
  "$root/bin" \
  "$root/config" \
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
