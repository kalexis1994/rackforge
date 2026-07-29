#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
artupy_root="${ARTUPY_ROOT:-$HOME/artupy}"
binary="$artupy_root/bin/nuked-sc55"
probe="$artupy_root/bin/artupy-nuked-probe"
model="${ARTUPY_NUKED_MODEL:-mk2}"
audio_device="${ARTUPY_AUDIO_DEVICE:-plughw:CARD=USB,DEV=0}"
page_size="${ARTUPY_NUKED_PAGE_SIZE:-512}"
page_count="${ARTUPY_NUKED_PAGE_COUNT:-8}"

if [[ ! -x "$binary" ]]; then
  printf 'No existe %s. Ejecutá build.sh primero.\n' "$binary" >&2
  exit 1
fi

if [[ ! -x "$probe" ]]; then
  printf 'No existe %s. Ejecutá build.sh primero.\n' "$probe" >&2
  exit 1
fi

bash "$script_dir/check-roms.sh" "$model"

if ! lsusb -d 1235:8211 >/dev/null; then
  printf '%s\n' 'No se encontró la Scarlett Solo USB 1235:8211.' >&2
  exit 1
fi

if ! aconnect -i | grep -Fq 'KL Essential 61 mk3 MIDI'; then
  printf '%s\n' 'No se encontró la entrada MIDI principal del KeyLab.' >&2
  exit 1
fi

midi_port="${ARTUPY_NUKED_MIDI_PORT:-$(
  "$probe" --find-midi 'KL Essential 61 mk3 MIDI'
)}"

case "$model" in
  mk2) model_flag=-mk2 ;;
  mk1) model_flag=-mk1 ;;
esac

export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-dummy}"
export SDL_AUDIODRIVER="${SDL_AUDIODRIVER:-alsa}"
export AUDIODEV="$audio_device"

printf 'engine=Nuked-SC55 model=%s midi_port=%s audio=%s\n' \
  "$model" "$midi_port" "$audio_device"

exec "$binary" \
  "$model_flag" \
  -gs \
  "-p:$midi_port" \
  "-ab:$page_size:$page_count"
