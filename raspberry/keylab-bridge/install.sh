#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current/keylab-bridge}"
service_source="${RACKFORGE_SERVICE_SOURCE:-$root/current/systemd/rackforge-display.service}"

test -x "$source_root/target/release/rackforge-bridge"
test -f "$service_source"

install -d "$root/bin"
install -m 0755 \
  "$source_root/target/release/rackforge-bridge" \
  "$root/bin/rackforge-bridge.new"
mv "$root/bin/rackforge-bridge.new" "$root/bin/rackforge-bridge"

sudo install -m 0644 \
  "$service_source" \
  /etc/systemd/system/rackforge-display.service
sudo systemctl daemon-reload
sudo systemctl enable rackforge-display.service
sudo systemctl restart rackforge-display.service

printf 'DISPLAY_SERVICE_INSTALLED binary=%s service=%s\n' \
  "$root/bin/rackforge-bridge" \
  "rackforge-display.service"
