#!/usr/bin/env bash
set -euo pipefail

mode="${1:-plugin}"
root="${ARTUPY_ROOT:-/home/kalex/artupy}"
legacy_pid_file="$root/state/scva-live.pid"
plugin_pid_file="$root/state/artupy-core-live.pid"
legacy_log="$root/logs/scva-live.log"
plugin_log="$root/logs/artupy-core-live.log"

legacy_binary="$root/bin/artupy-scva-live"
plugin_binary="$root/bin/artupy-core"
plugin_package="$root/plugins/roland-scva"
rendered_bank="$root/share/rendered-piano-v1"

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
  : >"$plugin_log"
  nohup "$plugin_binary" live "$plugin_package" \
    --resource "rendered-bank=$rendered_bank" \
    --preset scva.piano-1 \
    >"$plugin_log" 2>&1 </dev/null &
  printf '%s\n' "$!" >"$plugin_pid_file"
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
    if grep -q '^READY_TO_PLAY$' "$log_file"; then
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
    test -x "$legacy_binary"
    test -x "$plugin_binary"
    test -d "$rendered_bank"
    test -f "$plugin_package/artupy-plugin.toml"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    if ! start_plugin || ! wait_ready "$plugin_pid_file" "$plugin_log"; then
      stop_verified "$plugin_pid_file" "$plugin_binary" || true
      start_legacy
      wait_ready "$legacy_pid_file" "$legacy_log"
      printf 'PLUGIN_START_FAILED rollback=legacy\n' >&2
      exit 1
    fi
    printf 'LIVE_ENGINE_SELECTED engine=plugin pid=%s\n' \
      "$(<"$plugin_pid_file")"
    ;;
  legacy)
    test -x "$legacy_binary"
    stop_verified "$plugin_pid_file" "$plugin_binary"
    stop_verified "$legacy_pid_file" "$legacy_binary"
    start_legacy
    wait_ready "$legacy_pid_file" "$legacy_log"
    printf 'LIVE_ENGINE_SELECTED engine=legacy pid=%s\n' \
      "$(<"$legacy_pid_file")"
    ;;
  *)
    printf 'usage: %s plugin|legacy\n' "$0" >&2
    exit 2
    ;;
esac
