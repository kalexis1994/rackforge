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

case "$(uname -m)" in
  aarch64|arm64) platform="linux-aarch64" ;;
  x86_64|amd64) platform="linux-x86-64" ;;
  *)
    printf 'Unsupported Linux controller package architecture: %s\n' "$(uname -m)" >&2
    exit 2
    ;;
esac

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

install -d "$output/bin/$platform"
install -m 0755 \
  "$binary" \
  "$output/bin/$platform/rackforge-arturia-keylab-essential-mk3-driver"

driver_digest="$(
  sha256sum "$output/bin/$platform/rackforge-arturia-keylab-essential-mk3-driver" |
    awk '{print $1}'
)"
package_digest="$(
  {
    cat "$template"
    printf '\0%s\0' "$platform"
    cat "$binary"
  } | sha256sum | awk '{print $1}'
)"
base_version="$(awk -F '"' '/^version = "/ { print $2; exit }' "$template")"
base_version="${base_version%%+*}"
bundled_version="${base_version}+bundled.${package_digest:0:12}"
sed \
  "0,/^version = \"[^\"]*\"/s//version = \"$bundled_version\"/" \
  "$template" > "$output/rackforge-controller.toml"
{
  printf '\n[integrity.sha256]\n'
  printf '%s = "%s"\n' "$platform" "$driver_digest"
} >>"$output/rackforge-controller.toml"

printf 'CONTROLLER_PACKAGE_BUILT path=%s version=%s sha256=%s\n' \
  "$output" "$bundled_version" "$driver_digest"
