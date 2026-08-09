#!/usr/bin/env bash
set -euo pipefail

initial_root="${RACKFORGE_ROOT:-${HOME:?HOME is required}/rackforge}"
source_root="${RACKFORGE_SOURCE:-$initial_root/current}"
source "$source_root/platforms/raspberry-pi/scripts/lib/install-env.sh"
rackforge_resolve_install_environment
root="$RACKFORGE_ROOT_RESOLVED"
source_root="${RACKFORGE_SOURCE:-$root/current}"
host_binary="$source_root/target/release/rackforge-controller-host"
driver_manifest="$source_root/hardware/keylab-bridge/Cargo.toml"
package_builder="$source_root/hardware/controllers/arturia-keylab-essential-mk3/build-package.sh"
service_source="$source_root/platforms/raspberry-pi/systemd/rackforge-controller-host.service"
temporary_root=""

test -f "$driver_manifest"

# The process driver statically links RackForge's shared API crates. Always
# rebuild it from the same source tree as the host so a session schema bump
# cannot leave an apparently healthy controller service in a retry loop.
cargo build --release \
  --manifest-path "$driver_manifest" \
  --bin rackforge-arturia-keylab-essential-mk3-driver
cargo build --release \
  --manifest-path "$source_root/Cargo.toml" \
  --bin rackforge-controller-host

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

rackforge_render_systemd_unit \
  "$service_source" \
  /etc/systemd/system/rackforge-controller-host.service
sudo systemctl daemon-reload
sudo systemctl disable --now rackforge-display.service 2>/dev/null || true
sudo systemctl enable rackforge-controller-host.service
sudo systemctl restart rackforge-controller-host.service

printf 'CONTROLLER_RUNTIME_INSTALLED root=%s\n' "$root/controllers"
