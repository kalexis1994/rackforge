#!/usr/bin/env bash

rackforge_resolve_install_environment() {
  local requested_user passwd_entry
  requested_user="${RACKFORGE_USER:-}"
  if [[ -z "$requested_user" && -n "${SUDO_USER:-}" && "$SUDO_USER" != root ]]; then
    requested_user="$SUDO_USER"
  fi
  if [[ -z "$requested_user" ]]; then
    requested_user="$(id -un)"
  fi
  if [[ "$requested_user" == root && -z "${RACKFORGE_USER:-}" ]]; then
    printf 'Run as the target desktop user or set RACKFORGE_USER explicitly.\n' >&2
    return 2
  fi

  passwd_entry="$(getent passwd "$requested_user" || true)"
  if [[ -z "$passwd_entry" ]]; then
    printf 'RackForge user %s does not exist.\n' "$requested_user" >&2
    return 2
  fi

  RACKFORGE_USER_RESOLVED="$requested_user"
  RACKFORGE_GROUP_RESOLVED="$(id -gn "$requested_user")"
  RACKFORGE_HOME_RESOLVED="$(cut -d: -f6 <<<"$passwd_entry")"
  RACKFORGE_ROOT_RESOLVED="${RACKFORGE_ROOT:-$RACKFORGE_HOME_RESOLVED/rackforge}"
  if [[ "$RACKFORGE_ROOT_RESOLVED" != /* || "$RACKFORGE_ROOT_RESOLVED" =~ [[:space:]] ]]; then
    printf 'RACKFORGE_ROOT must be an absolute path without whitespace.\n' >&2
    return 2
  fi
  export RACKFORGE_USER_RESOLVED RACKFORGE_GROUP_RESOLVED
  export RACKFORGE_HOME_RESOLVED RACKFORGE_ROOT_RESOLVED
}

rackforge_render_systemd_unit() {
  local source="$1" destination="$2" escaped_user escaped_group escaped_root temporary
  escaped_user="$(sed 's/[&|\\]/\\&/g' <<<"$RACKFORGE_USER_RESOLVED")"
  escaped_group="$(sed 's/[&|\\]/\\&/g' <<<"$RACKFORGE_GROUP_RESOLVED")"
  escaped_root="$(sed 's/[&|\\]/\\&/g' <<<"$RACKFORGE_ROOT_RESOLVED")"
  temporary="$(mktemp)"
  sed \
    -e "s|@RACKFORGE_USER@|$escaped_user|g" \
    -e "s|@RACKFORGE_GROUP@|$escaped_group|g" \
    -e "s|@RACKFORGE_ROOT@|$escaped_root|g" \
    "$source" >"$temporary"
  if grep -Eq '@RACKFORGE_(USER|GROUP|ROOT)@' "$temporary"; then
    rm -f "$temporary"
    printf 'Unresolved RackForge systemd template token in %s.\n' "$source" >&2
    return 1
  fi
  sudo install -m 0644 "$temporary" "$destination"
  rm -f "$temporary"
}

# The machine's LAN address, for the line that says where the Web interface
# is. Not every distribution ships `hostname` (SteamOS has none, and the line
# then named 127.0.0.1); every one ships iproute2's `ip`.
rackforge_lan_address() {
  local address=""
  address="$(hostname -I 2>/dev/null | awk '{print $1}')" || true
  if [[ -z "$address" ]]; then
    address="$(ip -4 route get 1.1.1.1 2>/dev/null \
      | awk '{for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit }}')" || true
  fi
  printf '%s' "$address"
}
