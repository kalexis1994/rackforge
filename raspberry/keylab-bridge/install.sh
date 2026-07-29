#!/usr/bin/env bash
set -euo pipefail

root="${ARTUPY_ROOT:-/home/kalex/artupy}"
source_root="${ARTUPY_SOURCE:-$root/current/keylab-bridge}"
service_source="${ARTUPY_SERVICE_SOURCE:-$root/current/systemd/artupy-display.service}"

test -x "$source_root/target/release/artupy-bridge"
test -f "$service_source"

install -d "$root/bin"
install -m 0755 \
  "$source_root/target/release/artupy-bridge" \
  "$root/bin/artupy-bridge.new"
mv "$root/bin/artupy-bridge.new" "$root/bin/artupy-bridge"

sudo install -m 0644 \
  "$service_source" \
  /etc/systemd/system/artupy-display.service
sudo systemctl daemon-reload
sudo systemctl enable artupy-display.service
sudo systemctl restart artupy-display.service

printf 'DISPLAY_SERVICE_INSTALLED binary=%s service=%s\n' \
  "$root/bin/artupy-bridge" \
  "artupy-display.service"
