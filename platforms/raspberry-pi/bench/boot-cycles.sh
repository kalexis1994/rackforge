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

for cycle in $(seq 1 "$count"); do
  ssh -o ConnectTimeout=5 "$host" 'sudo systemctl reboot' || true
  # Gone: SSH no longer connects.
  while ssh -o ConnectTimeout=2 -o BatchMode=yes "$host" true 2>/dev/null; do
    sleep 0.5
  done
  down="$(now)"
  # Back: SSH answers.
  until ssh -o ConnectTimeout=2 -o BatchMode=yes "$host" true 2>/dev/null; do
    sleep 0.5
  done
  up="$(now)"
  # Let the boot finish before reading it.
  sleep 30
  wall="$(awk -v up="$up" -v down="$down" 'BEGIN { printf "%.2f", up - down }')"
  ssh "$host" "$bench boot-time --out $records/boot-$label-$cycle.json >/dev/null && \
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
