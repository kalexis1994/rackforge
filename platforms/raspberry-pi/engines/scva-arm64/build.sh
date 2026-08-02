#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
rackforge_root=${RACKFORGE_ROOT:-"$HOME/rackforge"}

cargo build \
    --locked \
    --release \
    --manifest-path "$root/Cargo.toml"

install -d "$rackforge_root/bin"
install -m 0755 \
    "$root/target/release/rackforge-scva-bank" \
    "$rackforge_root/bin/rackforge-scva-bank"

printf 'installed=%s\n' "$rackforge_root/bin/rackforge-scva-bank"
