#!/usr/bin/env bash
# Storage: what the instrument writes to its card while it plays.
#
# The journal lives in RAM, capped, so the engine's telemetry is never
# written to the card and a boot's log cannot fill memory. Sessions,
# configuration, plugins and data are RackForge's own files and stay where
# they are.

module_storage_declare() {
  want_file /etc/systemd/journald.conf.d/90-rackforge.conf 644 <<'EOF'
# Installed by rackforge-pi (dedicated profile). The journal is kept in RAM
# and lost at power-off: the instrument writes nothing to its card to log.
[Journal]
Storage=volatile
RuntimeMaxUse=48M
RuntimeMaxFileSize=8M
EOF
}

module_storage_after() {
  [[ -n "$DEDICATED_ROOT" ]] && return 0
  "$DEDICATED_SYSTEMCTL" restart systemd-journald
}

module_storage_check() {
  [[ -n "$DEDICATED_ROOT" ]] && return 0
  if journalctl --header 2>/dev/null | grep -q '^File path: /var/log/journal'; then
    echo "  NOT APPLIED [storage] the journal is still written to the card"
    return 1
  fi
  return 0
}
