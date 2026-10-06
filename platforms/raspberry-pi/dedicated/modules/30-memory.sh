#!/usr/bin/env bash
# Memory: the engine locks itself in RAM (mlockall) and must be allowed to;
# nothing it touches may be swapped out from under it.

module_memory_declare() {
  # The unit grants the engine its limits. These are for a RackForge
  # process started by hand from a login, which goes through PAM instead.
  want_file /etc/security/limits.d/rackforge-audio.conf 644 <<'EOF'
# Installed by rackforge-pi (dedicated profile): realtime priority and
# locked memory for the RackForge engine when started from a login.
@rackforge - rtprio 95
@rackforge - memlock unlimited
EOF

  # No swap. The engine's memory is locked and cannot be swapped anyway; what
  # swap would still do is write other processes' pages to the card in the
  # middle of a performance. The path is the one the earlier appliance
  # optimiser used, so a machine it touched ends with one file, not two.
  if [[ -d "$(path /etc/rpi)" ]]; then
    want_file /etc/rpi/swap.conf.d/10-rackforge-realtime.conf 644 <<'EOF'
# Installed by rackforge-pi (dedicated profile). `rackforge-pi revert`
# restores the image's own swap policy.
[Main]
Mechanism=none
EOF
  fi
  [[ "$(unit_state dphys-swapfile.service)" == not-found ]] ||
    want_unit dphys-swapfile.service disabled now
}

module_memory_after() {
  # A policy file changes the next boot; the swap in use now goes now.
  if [[ -z "$DEDICATED_ROOT" ]] && grep -q . <(tail -n +2 /proc/swaps); then
    swapoff --all
  fi
}

module_memory_check() {
  [[ -n "$DEDICATED_ROOT" ]] && return 0
  if grep -q . <(tail -n +2 /proc/swaps); then
    echo "  NOT APPLIED [memory] swap is in use: $(tail -n +2 /proc/swaps | awk '{print $1}' | tr '\n' ' ')"
    return 1
  fi
  local pid locked
  pid="$("$DEDICATED_SYSTEMCTL" show -p MainPID --value rackforge-audio 2>/dev/null || true)"
  if [[ -n "$pid" && "$pid" != 0 ]]; then
    locked="$(awk '/^VmLck:/ { print $2 }' "/proc/$pid/status" 2>/dev/null || echo 0)"
    if [[ "${locked:-0}" -eq 0 ]]; then
      echo "  NOT APPLIED [memory] the engine's memory is not locked"
      return 1
    fi
  fi
  return 0
}
