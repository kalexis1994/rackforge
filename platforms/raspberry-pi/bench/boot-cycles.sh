#!/usr/bin/env bash
# Reboots a RackForge Pi N times and records how long each boot took to make
# sound. Run from another machine that reaches the Pi over SSH as a user
# with passwordless sudo:
#
#   boot-cycles.sh HOST LABEL [COUNT]
#
# Per boot it keeps the Pi's own record (rackforge-pi-bench boot-time: times
# from the kernel's start) and, from this side, the wall time from the Pi
# going silent to SSH answering again. The kernel cannot see the firmware
# that runs before it; that difference, less the Pi's own SSH time, is the
# firmware and bootloader, approximately.
set -euo pipefail

host="${1:?host}"
label="${2:?label}"
count="${3:-10}"
bench='~/bench/rackforge-pi-bench'
records='~/bench/records'

now() { date +%s.%N; }

# Extra ssh options for every connection, from BENCH_SSH_OPTIONS -- for
# instance `-o HostName=192.168.1.17` where the Pi's mDNS name resolves
# only now and then.
ssh() {
  # shellcheck disable=SC2086
  command ssh ${BENCH_SSH_OPTIONS:-} "$@"
}

# An ssh that fails to connect -- an mDNS name that does not resolve for a
# moment, which happens -- is tried again rather than ending the run.
remote() {
  local attempt
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    ssh -o ConnectTimeout=5 "$host" "$@" && return 0
    local status=$?
    ((status == 255)) || return "$status"
    sleep 2
  done
  return 255
}

boot_id() {
  ssh -o ConnectTimeout=5 -o BatchMode=yes "$host" cat /proc/sys/kernel/random/boot_id 2>/dev/null
}

for cycle in $(seq 1 "$count"); do
  # The boot this cycle starts from. A name that briefly fails to resolve
  # looks exactly like a Pi that went down; only a new boot id proves one.
  previous=""
  until previous="$(boot_id)" && [[ -n "$previous" ]]; do sleep 1; done
  ssh -o ConnectTimeout=5 "$host" 'sudo systemctl reboot' || true
  # Gone: SSH no longer connects.
  while ssh -o ConnectTimeout=2 -o BatchMode=yes "$host" true 2>/dev/null; do
    sleep 0.5
  done
  down="$(now)"
  # Back: SSH answers, from a boot that is not the one this cycle left. A
  # reboot whose ssh never connected left the same boot running; asked again.
  asked="$(date +%s)"
  until current="$(boot_id)" && [[ -n "$current" && "$current" != "$previous" ]]; do
    if [[ "$current" == "$previous" ]] && (($(date +%s) - asked > 90)); then
      ssh -o ConnectTimeout=5 "$host" 'sudo systemctl reboot' || true
      asked="$(date +%s)"
      down="$(now)"
    fi
    sleep 0.5
  done
  up="$(now)"
  # Let the boot finish before reading it.
  sleep 30
  wall="$(awk -v up="$up" -v down="$down" 'BEGIN { printf "%.2f", up - down }')"
  remote "$bench boot-time --out $records/boot-$label-$cycle.json >/dev/null && \
    python3 - $records/boot-$label-$cycle.json $wall <<'EOF'
import json, sys
path, wall = sys.argv[1], float(sys.argv[2])
record = json.load(open(path))
record['wall_down_to_ssh_s'] = round(wall, 2)
if record.get('ssh_listening_s') is not None:
    record['before_kernel_s_estimate'] = round(wall - record['ssh_listening_s'], 2)
json.dump(record, open(path, 'w'), indent=2)
print(f\"cycle {path}: ready {record.get('engine_ready_s')} s after kernel start, \"
      f\"ssh {record.get('ssh_listening_s')} s, wall down-to-ssh {wall:.1f} s\")
EOF"
done
