#!/usr/bin/env bash
set -euo pipefail

rom_dir="${RACKFORGE_NUKED_ROM_DIR:-$HOME/rackforge/share/nuked-sc55}"
model="${1:-mk2}"

case "$model" in
  mk2)
    required=(rom1.bin rom2.bin rom_sm.bin waverom1.bin waverom2.bin)
    ;;
  mk1)
    required=(
      sc55_rom1.bin
      sc55_rom2.bin
      sc55_waverom1.bin
      sc55_waverom2.bin
      sc55_waverom3.bin
    )
    ;;
  *)
    printf 'Modelo no soportado por el launcher inicial: %s\n' "$model" >&2
    exit 2
    ;;
esac

missing=0
for name in "${required[@]}"; do
  path="$rom_dir/$name"
  if [[ -s "$path" ]]; then
    printf 'ok  %10s bytes  %s\n' "$(stat -c %s "$path")" "$name"
  else
    printf 'falta                 %s\n' "$name" >&2
    missing=1
  fi
done

if ((missing)); then
  printf 'ROM set incompleto en %s\n' "$rom_dir" >&2
  exit 1
fi
