#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/install-env.sh"
rackforge_resolve_install_environment

root="$RACKFORGE_ROOT_RESOLVED"
source_root="${RACKFORGE_SOURCE:-$root/current}"
systemd_root="${RACKFORGE_SYSTEMD_SOURCE:-$root/current/platforms/raspberry-pi/systemd}"
service_user="$RACKFORGE_USER_RESOLVED"
optimize="${1:-}"

if [[ -n "$optimize" && "$optimize" != --optimize ]]; then
  printf 'usage: %s [--optimize]\n' "$0" >&2
  exit 2
fi

test -x "$source_root/target/release/rackforge-platform-host"
test -x "$source_root/target/release/rackforge-controller-host"
test -x "$source_root/target/release/rackforge-core"
test -x "$source_root/target/release/rackforge-web"
test -f "$source_root/config/audio.toml"
test -f "$systemd_root/rackforge-platform-host.service"
test -f "$systemd_root/rackforge-controller-host.service"
test -f "$systemd_root/rackforge-web.service"
test -f "$systemd_root/rackforge-audio.service"
test -f "$systemd_root/rackforge-audio.path"

sudo groupadd --system --force rackforge
sudo usermod -a -G rackforge "$service_user"

install -d "$root/bin" "$root/config" "$root/data" "$root/state" "$root/logs"
install -m 0755 \
  "$source_root/target/release/rackforge-platform-host" \
  "$root/bin/rackforge-platform-host.new"
mv "$root/bin/rackforge-platform-host.new" "$root/bin/rackforge-platform-host"

install -m 0755 \
  "$source_root/target/release/rackforge-core" \
  "$root/bin/rackforge-core.new"
mv "$root/bin/rackforge-core.new" "$root/bin/rackforge-core"

install -m 0755 \
  "$source_root/target/release/rackforge-controller-host" \
  "$root/bin/rackforge-controller-host.new"
mv "$root/bin/rackforge-controller-host.new" "$root/bin/rackforge-controller-host"

install -m 0755 \
  "$source_root/target/release/rackforge-web" \
  "$root/bin/rackforge-web.new"
mv "$root/bin/rackforge-web.new" "$root/bin/rackforge-web"

if [[ ! -f "$root/config/audio.toml.example" ]]; then
  install -m 0644 \
    "$source_root/config/audio.toml" \
    "$root/config/audio.toml.example"
fi

rackforge_render_systemd_unit \
  "$systemd_root/rackforge-platform-host.service" \
  /etc/systemd/system/rackforge-platform-host.service
rackforge_render_systemd_unit \
  "$systemd_root/rackforge-controller-host.service" \
  /etc/systemd/system/rackforge-controller-host.service
rackforge_render_systemd_unit \
  "$systemd_root/rackforge-web.service" \
  /etc/systemd/system/rackforge-web.service
rackforge_render_systemd_unit \
  "$systemd_root/rackforge-audio.service" \
  /etc/systemd/system/rackforge-audio.service
rackforge_render_systemd_unit \
  "$systemd_root/rackforge-audio.path" \
  /etc/systemd/system/rackforge-audio.path
sudo systemctl daemon-reload
sudo systemctl enable \
  rackforge-platform-host.service \
  rackforge-controller-host.service \
  rackforge-web.service \
  rackforge-audio.service \
  rackforge-audio.path
sudo systemctl restart \
  rackforge-platform-host.service \
  rackforge-controller-host.service \
  rackforge-web.service \
  rackforge-audio.path
if [[ -f "$root/config/audio.toml" ]]; then
  sudo systemctl restart rackforge-audio.service
else
  sudo systemctl stop rackforge-audio.service
fi

if [[ "$optimize" == --optimize ]]; then
  bash "$source_root/platforms/raspberry-pi/scripts/optimize-appliance.sh" apply
fi

lan_address="$(hostname -I 2>/dev/null | awk '{print $1}')"
printf 'APPLIANCE_RUNTIME_INSTALLED profile=detected audio=supervised web=http://%s:8787\n' \
  "${lan_address:-127.0.0.1}"
