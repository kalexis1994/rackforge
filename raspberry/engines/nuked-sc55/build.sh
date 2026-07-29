#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source_dir="$script_dir/vendor/Nuked-SC55"
rackforge_root="${RACKFORGE_ROOT:-$HOME/rackforge}"
build_dir="$rackforge_root/build/nuked-sc55"

if [[ ! -f "$source_dir/CMakeLists.txt" ]]; then
  printf '%s\n' \
    'Falta el submódulo Nuked-SC55. Ejecutá git submodule update --init --recursive.' \
    >&2
  exit 1
fi

if [[ ! -f "$source_dir/3rdparty/rtmidi/CMakeLists.txt" ]]; then
  printf '%s\n' \
    'Falta el submódulo RtMidi incluido por Nuked-SC55.' >&2
  exit 1
fi

cmake \
  -S "$source_dir" \
  -B "$build_dir" \
  -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$rackforge_root"

cmake --build "$build_dir" --parallel
cmake --install "$build_dir"

c++ \
  -std=c++17 \
  -O2 \
  -D__LINUX_ALSA__ \
  -I"$source_dir/3rdparty/rtmidi" \
  $(sdl2-config --cflags) \
  "$script_dir/probe-runtime.cpp" \
  "$source_dir/3rdparty/rtmidi/RtMidi.cpp" \
  $(sdl2-config --libs) \
  -lasound \
  -lpthread \
  -o "$rackforge_root/bin/rackforge-nuked-probe"

printf 'binary=%s\n' "$rackforge_root/bin/nuked-sc55"
file "$rackforge_root/bin/nuked-sc55"
printf 'probe=%s\n' "$rackforge_root/bin/rackforge-nuked-probe"
file "$rackforge_root/bin/rackforge-nuked-probe"
