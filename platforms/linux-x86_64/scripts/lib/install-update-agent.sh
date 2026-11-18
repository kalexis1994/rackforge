#!/usr/bin/env bash
# Called after the managed units have been rendered. The worker changes files
# as the host owner; its root supervisor only controls those fixed units.
rackforge_lock_installation() {
  install -d "$RACKFORGE_ROOT_RESOLVED"
  # install-release.sh passes this very open file description to its child.
  if [[ "$(readlink "/proc/$$/fd/9" 2>/dev/null || true)" != "$(realpath -m "$RACKFORGE_ROOT_RESOLVED/.install.lock")" ]]; then
    exec 9>"$RACKFORGE_ROOT_RESOLVED/.install.lock"
  fi
  flock -n 9 || { printf 'Another RackForge installation or recovery is running.\n' >&2; return 1; }
  python3 - "$RACKFORGE_ROOT_RESOLVED/state/updates/activation.json" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
if path.exists() and json.loads(path.read_text())["status"]["phase"] not in ("idle", "installed", "rolled_back"):
    raise SystemExit("Recover the interrupted automatic update before installing another release.")
PY
}

rackforge_install_update_agent() {
  local source_root="$1" systemd_root="$2" temporary
  sudo install -d -m 0755 /usr/libexec /etc/rackforge-updater
  sudo install -o root -g root -m 0755 "$source_root/target/release/rackforge-updater" /usr/libexec/rackforge-update-agent.new
  sudo mv /usr/libexec/rackforge-update-agent.new /usr/libexec/rackforge-update-agent
  temporary="$(mktemp)"
  python3 - "$RACKFORGE_ROOT_RESOLVED" "$RACKFORGE_USER_RESOLVED" "$RACKFORGE_GROUP_RESOLVED" >"$temporary" <<'PY'
import json, sys
print(json.dumps(dict(root=sys.argv[1], user=sys.argv[2], group=sys.argv[3],
                     service_prefix="rackforge", runtime="/run/rackforge-installer", web_port=8787)))
PY
  sudo install -o root -g root -m 0644 "$temporary" /etc/rackforge-updater/agent.json
  rm -f "$temporary"
  rackforge_render_systemd_unit "$systemd_root/rackforge-installer.service" /etc/systemd/system/rackforge-installer.service
  for unit in rackforge-audio.service rackforge-audio.path rackforge-platform-host.service rackforge-controller-host.service rackforge-web.service rackforge-updater.service; do
    sudo install -d -m 0755 "/etc/systemd/system/$unit.d"
    printf '[Unit]\nRequires=rackforge-installer.service\nAfter=rackforge-installer.service\n' | sudo tee "/etc/systemd/system/$unit.d/updates.conf" >/dev/null
  done
  # The agent reads a durable installed-version marker, including on machines
  # bootstrapped from releases predating automatic updates.
  if [[ -f "$source_root/build-info.txt" ]]; then
    install -m 0644 "$source_root/build-info.txt" "$RACKFORGE_ROOT_RESOLVED/build-info.txt"
  fi
  sudo systemctl daemon-reload
  sudo systemctl enable rackforge-installer.service
  sudo systemctl restart rackforge-installer.service
}
