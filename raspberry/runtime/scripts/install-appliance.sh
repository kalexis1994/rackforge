#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current/runtime}"
systemd_root="${RACKFORGE_SYSTEMD_SOURCE:-$root/current/systemd}"
service_user="${RACKFORGE_USER:-kalex}"
optimize="${1:-}"

if [[ -n "$optimize" && "$optimize" != --optimize ]]; then
  printf 'usage: %s [--optimize]\n' "$0" >&2
  exit 2
fi

test -x "$source_root/target/release/rackforge-platform-host"
test -x "$source_root/target/release/rackforge-core"
test -f "$source_root/config/audio.toml"
test -f "$systemd_root/rackforge-platform-host.service"
test -f "$systemd_root/rackforge-audio.service"

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

if [[ ! -f "$root/config/audio.toml" ]]; then
  install -m 0644 "$source_root/config/audio.toml" "$root/config/audio.toml"
fi

sudo install -m 0644 \
  "$systemd_root/rackforge-platform-host.service" \
  /etc/systemd/system/rackforge-platform-host.service
sudo install -m 0644 \
  "$systemd_root/rackforge-audio.service" \
  /etc/systemd/system/rackforge-audio.service
sudo systemctl daemon-reload
sudo systemctl enable rackforge-platform-host.service rackforge-audio.service
sudo systemctl restart rackforge-platform-host.service

if [[ "$optimize" == --optimize ]]; then
  bash "$source_root/scripts/optimize-appliance.sh" apply
fi

printf 'APPLIANCE_RUNTIME_INSTALLED profile=detected audio=supervised\n'
