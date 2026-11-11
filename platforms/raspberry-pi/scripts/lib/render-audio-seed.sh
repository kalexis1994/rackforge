# Renders the startup audio configuration an installer leaves behind.
#
# `config/audio.toml` in the repository is a template, not a file to copy: it
# names @RACKFORGE_ROOT@ and @RACKFORGE_DEFAULT_PACKAGE@ because Core refuses
# a relative `package` or `data_root`, and because the Web host copies this
# seed as it stands the first time an instrument is activated. An unrendered
# copy therefore does not fail at install time -- it fails much later, as an
# engine that will not start and an interface stuck on "Waiting for Core".

# The bundled instrument the seed should point at: the newest installed
# version of the default package, so the rendered file runs as it stands
# rather than naming a version that has rotted.
rackforge_default_package() {
  local root="$1"
  local id="${2:-org.rackforge.concert-grand}"
  local newest
  newest="$(ls -d "$root/plugin-store/packages/$id"/*/ 2>/dev/null | sort -V | tail -n 1)"
  [[ -n "$newest" ]] || return 1
  printf '%s' "${newest%/}"
}

rackforge_render_audio_seed() {
  local source="$1"
  local destination="$2"
  local package escaped_root escaped_package temporary
  if ! package="$(rackforge_default_package "$RACKFORGE_ROOT_RESOLVED")"; then
    printf 'No bundled instrument found; %s not written.\n' "$destination" >&2
    return 0
  fi
  escaped_root="$(sed 's/[&|\]/\\&/g' <<<"$RACKFORGE_ROOT_RESOLVED")"
  escaped_package="$(sed 's/[&|\]/\\&/g' <<<"$package")"
  temporary="$(mktemp)"
  sed \
    -e "s|@RACKFORGE_ROOT@|$escaped_root|g" \
    -e "s|@RACKFORGE_DEFAULT_PACKAGE@|$escaped_package|g" \
    "$source" >"$temporary"
  # The same guard the systemd templates carry: a token that survived means a
  # file that looks installed and is not.
  if grep -Eq '@RACKFORGE_[A-Z_]+@' "$temporary"; then
    rm -f "$temporary"
    printf 'Unresolved RackForge template token in %s.\n' "$source" >&2
    return 1
  fi
  install -m 0644 "$temporary" "$destination"
  rm -f "$temporary"
}
