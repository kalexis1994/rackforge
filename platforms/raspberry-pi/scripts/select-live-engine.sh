#!/usr/bin/env bash
set -euo pipefail

mode="${1:-plugin}"
selection="${2:-}"
root="${RACKFORGE_ROOT:-${HOME:?HOME is required}/rackforge}"
legacy_pid_file="$root/state/scva-live.pid"
native_pid_file="$root/state/scva-native-live.pid"
plugin_pid_file="$root/state/rackforge-core-live.pid"
legacy_log="$root/logs/scva-live.log"
native_log="$root/logs/scva-native-live.log"
plugin_log="$root/logs/rackforge-core-live.log"

legacy_binary="$root/bin/rackforge-scva-live"
plugin_binary="$root/bin/rackforge-core"
plugin_package="$root/plugins/roland-scva"
rf_dls_plugin_package="$root/plugins/rf-dls"
rendered_bank="$root/share/rendered-piano-v1"
rf_dls_bank="$root/data/plugins/rf-dls/banks/gm.dls"

process_command() {
  local pid="$1"
  if [[ -r "/proc/$pid/cmdline" ]]; then
    tr '\0' ' ' <"/proc/$pid/cmdline"
  fi
}

stop_verified() {
  local pid_file="$1"
  local expected="$2"
  if [[ ! -f "$pid_file" ]]; then
    return 0
  fi
  local pid
  pid="$(<"$pid_file")"
  if ! kill -0 "$pid" 2>/dev/null; then
    rm -f "$pid_file"
    return 0
  fi
  local command
  command="$(process_command "$pid")"
  if [[ "$command" != *"$expected"* ]]; then
    printf 'Refusing to stop pid %s with unexpected command: %s\n' \
      "$pid" "$command" >&2
    return 1
  fi
  kill "$pid"
  for _ in {1..10}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$pid_file"
      return 0
    fi
    sleep 0.2
  done
  printf 'Process %s did not stop cleanly\n' "$pid" >&2
  return 1
}

start_legacy() {
  : >"$legacy_log"
  nohup "$legacy_binary" \
    --rendered-bank "$rendered_bank" \
    --gain 1.0 \
    >"$legacy_log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$legacy_pid_file"
}

start_plugin() {
  local preset="${1:-scva.piano-1}"
  : >"$plugin_log"
  nohup "$plugin_binary" live "$plugin_package" \
    --resource "rendered-bank=$rendered_bank" \
    --preset "$preset" \
    --data-root "$root/data" \
    >"$plugin_log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$plugin_pid_file"
}

start_rf_dls_plugin() {
  local preset="${1:-gm.piano-1}"
  : >"$plugin_log"
  nohup "$plugin_binary" live "$rf_dls_plugin_package" \
    --resource "dls-bank=$rf_dls_bank" \
    --preset "$preset" \
    --data-root "$root/data" \
    >"$plugin_log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$plugin_pid_file"
}

start_native() {
  local tone="${1:-390}"
  : >"$native_log"
  nohup "$legacy_binary" \
    --tone "$tone" \
    "$root/share/scva" \
    "$root/share/scva/control-v1" \
    >"$native_log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$native_pid_file"
}

wait_ready() {
  local pid_file="$1"
  local log_file="$2"
  local pid
  pid="$(<"$pid_file")"
  for _ in {1..30}; do
    if ! kill -0 "$pid" 2>/dev/null; then
      tail -n 30 "$log_file" >&2 || true
      return 1
    fi
    if grep -q '^READY_TO_PLAY' "$log_file"; then
      return 0
    fi
    sleep 0.25
  done
  printf 'Engine did not become ready in time\n' >&2
  tail -n 30 "$log_file" >&2 || true
  return 1
}

case "$mode" in
  plugin)
    preset="${selection:-scva.piano-1}"
    test -x "$legacy_binary"
    test -x "$plugin_binary"
    test -d "$rendered_bank"
    test -f "$plugin_package/rackforge-plugin.toml"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$native_pid_file" "$legacy_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    if ! start_plugin "$preset" || ! wait_ready "$plugin_pid_file" "$plugin_log"; then
      stop_verified "$plugin_pid_file" "$plugin_binary" || true
      start_legacy
      wait_ready "$legacy_pid_file" "$legacy_log"
      printf 'PLUGIN_START_FAILED rollback=legacy\n' >&2
      exit 1
    fi
    printf 'LIVE_ENGINE_SELECTED engine=plugin preset=%s pid=%s\n' \
      "$preset" "$(<"$plugin_pid_file")"
    ;;
  legacy)
    test -x "$legacy_binary"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$native_pid_file" "$legacy_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    start_legacy
    wait_ready "$legacy_pid_file" "$legacy_log"
    printf 'LIVE_ENGINE_SELECTED engine=legacy pid=%s\n' \
      "$(<"$legacy_pid_file")"
    ;;
  native)
    tone="${selection:-390}"
    test -x "$legacy_binary"
    test -d "$root/share/scva"
    test -d "$root/share/scva/control-v1"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$native_pid_file" "$legacy_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    if ! start_native "$tone" || ! wait_ready "$native_pid_file" "$native_log"; then
      stop_verified "$native_pid_file" "$legacy_binary" || true
      start_plugin scva.strings-1
      wait_ready "$plugin_pid_file" "$plugin_log"
      printf 'NATIVE_START_FAILED rollback=plugin preset=scva.strings-1\n' >&2
      exit 1
    fi
    printf 'LIVE_ENGINE_SELECTED engine=native tone=%s pid=%s\n' \
      "$tone" "$(<"$native_pid_file")"
    ;;
  rf-dls-plugin)
    preset="${selection:-gm.piano-1}"
    test -x "$plugin_binary"
    test -f "$rf_dls_plugin_package/rackforge-plugin.toml"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$native_pid_file" "$legacy_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    "$plugin_binary" plugin-init "$root/data" org.rackforge.rf-dls
    test -f "$rf_dls_bank"
    if ! start_rf_dls_plugin "$preset" || ! wait_ready "$plugin_pid_file" "$plugin_log"; then
      stop_verified "$plugin_pid_file" "$plugin_binary" || true
      printf 'RF_DLS_PLUGIN_START_FAILED\n' >&2
      exit 1
    fi
    printf 'LIVE_ENGINE_SELECTED engine=rf-dls-plugin preset=%s pid=%s\n' \
      "$preset" "$(<"$plugin_pid_file")"
    ;;
  *)
    printf \
      'usage: %s plugin [preset]|native [tone]|legacy|rf-dls-plugin [preset]\n' \
      "$0" >&2
    exit 2
    ;;
esac
