#!/usr/bin/env bash
set -eu

readonly focusrite_vendor="1235"
readonly focusrite_product="8211"

printf '%s\n' '--- USB audio esperado ---'
if ! lsusb -d "${focusrite_vendor}:${focusrite_product}"; then
    printf 'No se encontró la Focusrite %s:%s.\n' \
        "$focusrite_vendor" "$focusrite_product" >&2
    exit 1
fi

printf '%s\n' '--- Tarjetas ALSA ---'
cat /proc/asound/cards

printf '%s\n' '--- Reproducción ---'
aplay -l

printf '%s\n' '--- Captura ---'
arecord -l

card_number="$(
    awk '
        /^[[:space:]]*[0-9]+[[:space:]]+\[USB[[:space:]]*\]:/ {
            card = $1
            gsub(/[^0-9]/, "", card)
            print card
            exit
        }
    ' /proc/asound/cards
)"

if [ -z "$card_number" ]; then
    printf '%s\n' \
        'La Focusrite está en USB pero no existe la tarjeta ALSA CARD=USB.' >&2
    exit 1
fi

stream="/proc/asound/card${card_number}/stream0"
if [ ! -r "$stream" ]; then
    printf 'No se puede leer %s.\n' "$stream" >&2
    exit 1
fi

printf '%s\n' '--- Capacidades Focusrite ---'
cat "$stream"

printf '%s\n' '--- Mezclador Focusrite ---'
amixer -c USB scontrols

if command -v vcgencmd >/dev/null; then
    printf '%s\n' '--- Alimentación Raspberry ---'
    vcgencmd get_throttled
fi
