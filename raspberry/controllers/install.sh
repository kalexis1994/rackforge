#!/usr/bin/env bash
set -euo pipefail

root="${RACKFORGE_ROOT:-/home/kalex/rackforge}"
source_root="${RACKFORGE_SOURCE:-$root/current}"
host_binary="$source_root/runtime/target/release/rackforge-controller-host"
package_builder="$source_root/controllers/arturia-keylab-essential-mk3/build-package.sh"
service_source="$source_root/systemd/rackforge-controller-host.service"
temporary_root=""

cleanup() {
  case "$temporary_root" in
    "$root"/build/controller-install.*) rm -rf -- "$temporary_root" ;;
  esac
}
trap cleanup EXIT

test -x "$host_binary"
test -f "$package_builder"
test -f "$service_source"
install -d "$root/bin" "$root/controllers" "$root/build"
temporary_root="$(mktemp -d "$root/build/controller-install.XXXXXX")"
package_path="$temporary_root/org.rackforge.arturia-keylab-essential-mk3.rfcontroller"

install -m 0755 "$host_binary" "$root/bin/rackforge-controller-host.new"
mv "$root/bin/rackforge-controller-host.new" "$root/bin/rackforge-controller-host"

bash "$package_builder" "$package_path"
"$root/bin/rackforge-controller-host" verify "$package_path"
"$root/bin/rackforge-controller-host" install \
  "$package_path" \
  --root "$root/controllers" \
  --trust official
"$root/bin/rackforge-controller-host" conformance \
  org.rackforge.arturia-keylab-essential-mk3 \
  --root "$root/controllers"

sudo install -m 0644 \
  "$service_source" \
  /etc/systemd/system/rackforge-controller-host.service
sudo systemctl daemon-reload
sudo systemctl disable --now rackforge-display.service 2>/dev/null || true
sudo systemctl enable rackforge-controller-host.service
sudo systemctl restart rackforge-controller-host.service

printf 'CONTROLLER_RUNTIME_INSTALLED root=%s\n' "$root/controllers"
