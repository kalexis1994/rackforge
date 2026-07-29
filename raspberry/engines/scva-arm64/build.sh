#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
artupy_root=${ARTUPY_ROOT:-"$HOME/artupy"}

cargo build \
    --locked \
    --release \
    --manifest-path "$root/Cargo.toml"

install -d "$artupy_root/bin"
install -m 0755 \
    "$root/target/release/artupy-scva-bank" \
    "$artupy_root/bin/artupy-scva-bank"

printf 'installed=%s\n' "$artupy_root/bin/artupy-scva-bank"
