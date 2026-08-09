#!/usr/bin/env bash
set -euo pipefail

source_root="${RACKFORGE_SOURCE:-${HOME:?HOME is required}/rackforge/current}"
output="${1:-${RACKFORGE_ROOT:-$HOME/rackforge}/build/org.rackforge.arturia-keylab-essential-mk3.rfcontroller}"
template="$source_root/hardware/controllers/arturia-keylab-essential-mk3/package/rackforge-controller.toml"
# The driver is a member of the root workspace, so cargo writes it to the
# shared target directory rather than beside its own manifest. This pointed at
# the latter, which only existed while the driver was a standalone package: the
# sanctioned install path has been failing at this line ever since it joined
# the workspace, which is how hand-copied binaries came to be the way anything
# reached the device.
target_root="${CARGO_TARGET_DIR:-$source_root/target}"
binary="$target_root/release/rackforge-arturia-keylab-essential-mk3-driver"

test -f "$template"
test -x "$binary"
if [[ "$output" != *.rfcontroller ]]; then
  printf 'Controller package output must end in .rfcontroller\n' >&2
  exit 2
fi
if [[ -e "$output" ]]; then
  printf 'Refusing to overwrite existing package %s\n' "$output" >&2
  exit 2
fi

install -d "$output/bin/linux-aarch64"
install -m 0644 "$template" "$output/rackforge-controller.toml"
install -m 0755 \
  "$binary" \
  "$output/bin/linux-aarch64/rackforge-arturia-keylab-essential-mk3-driver"

digest="$(
  sha256sum "$output/bin/linux-aarch64/rackforge-arturia-keylab-essential-mk3-driver" |
    awk '{print $1}'
)"
{
  printf '\n[integrity.sha256]\n'
  printf 'linux-aarch64 = "%s"\n' "$digest"
} >>"$output/rackforge-controller.toml"

printf 'CONTROLLER_PACKAGE_BUILT path=%s sha256=%s\n' "$output" "$digest"
