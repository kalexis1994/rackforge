#!/usr/bin/env bash
# The resource engine behind `rackforge-pi`.
#
# Modules never change the system themselves. They declare what they want
# -- a file with some content, a unit in some state, a line in config.txt, a
# token on the kernel command line -- and this engine compares each wish
# with the system as it is, changes what differs, and keeps what it
# replaced:
#
# - The original of anything is recorded once, before the first change, and
#   never overwritten afterwards, so a second run, an interrupted run or a
#   newer profile cannot lose the state from before RackForge.
# - Whether something is applied is always read from the system. The state
#   directory holds originals and a journal, never a verdict.
# - A wish a newer profile no longer makes is reverted on the next apply.
#
# Every path is under $DEDICATED_ROOT (empty on a real system) and every
# systemctl call goes through $DEDICATED_SYSTEMCTL, so the self-test can run
# the whole engine against a scratch tree.

DEDICATED_ROOT="${DEDICATED_ROOT:-}"
DEDICATED_SYSTEMCTL="${DEDICATED_SYSTEMCTL:-systemctl}"
STATE_DIR="$DEDICATED_ROOT/var/lib/rackforge/dedicated"
ORIGINALS="$STATE_DIR/originals"
JOURNAL="$STATE_DIR/journal"
STAGING=""

# Declared resources, one record each: TYPE<US>MODULE<US>ID<US>ARGS...
declare -a RESOURCES=()
US=$'\x1f'
CURRENT_MODULE=""
SYSTEMD_CHANGED=0

say() { printf '%s\n' "$*"; }
warn() { printf 'rackforge-pi: %s\n' "$*" >&2; }
die() {
  warn "$*"
  exit 1
}

path() { printf '%s%s' "$DEDICATED_ROOT" "$1"; }

# Called directly, never in a command substitution: a subshell's STAGING
# and EXIT trap would both be gone, the directory with them.
ensure_staging() {
  if [[ -z "$STAGING" ]]; then
    STAGING="$(mktemp -d)"
    trap 'rm -rf "$STAGING"' EXIT
  fi
}

# --- declaring -------------------------------------------------------------

# want_file TARGET MODE < content
want_file() {
  local target="$1" mode="$2" staged
  ensure_staging
  staged="$STAGING/$(printf '%s' "$target" | sha256sum | cut -c1-16)"
  cat >"$staged"
  RESOURCES+=("file$US$CURRENT_MODULE${US}file:$target$US$target$US$mode$US$staged")
}

# want_absent TARGET: the file must not exist.
want_absent() {
  RESOURCES+=("absent$US$CURRENT_MODULE${US}file:$1$US$1")
}

# want_exists TARGET: a flag file whose presence is all that matters. One
# that exists already is left as it is; a missing one is created empty.
want_exists() {
  RESOURCES+=("exists$US$CURRENT_MODULE${US}file:$1$US$1")
}

# want_unit UNIT enabled|disabled|masked [now]
want_unit() {
  RESOURCES+=("unit$US$CURRENT_MODULE${US}unit:$1$US$1$US$2$US${3:-}")
}

# want_line FILE LINE: an exact line, kept in the file's [all] section at
# its end. For config.txt, whose other lines belong to the user and the
# image.
want_line() {
  RESOURCES+=("line$US$CURRENT_MODULE${US}line:$1:$2$US$1$US$2")
}

# want_token TOKEN present|absent: a word on the kernel command line.
want_token() {
  RESOURCES+=("token$US$CURRENT_MODULE${US}token:$1$US$1$US$2")
}

# --- probing: does the system already match? --------------------------------

unit_state() {
  local state
  state="$("$DEDICATED_SYSTEMCTL" is-enabled "$1" 2>/dev/null)" || true
  printf '%s' "${state:-not-found}"
}

cmdline_file() {
  if [[ -f "$(path /boot/firmware/cmdline.txt)" ]]; then
    path /boot/firmware/cmdline.txt
  else
    path /boot/cmdline.txt
  fi
}

has_token() {
  tr ' ' '\n' <"$(cmdline_file)" | grep -qxF -- "$1"
}

# Prints nothing when the resource matches, otherwise what would change.
probe() {
  local type="$1" target="$3"
  case "$type" in
    file)
      local mode="$4" staged="$5" file
      file="$(path "$target")"
      if [[ ! -f "$file" ]]; then
        say "create $target"
      elif ! cmp -s "$file" "$staged"; then
        say "rewrite $target"
      elif [[ "$(stat -c %a "$file")" != "$mode" ]]; then
        say "chmod $mode $target"
      fi
      ;;
    absent)
      [[ -e "$(path "$target")" ]] && say "remove $target"
      ;;
    exists)
      [[ -e "$(path "$target")" ]] || say "create $target"
      ;;
    unit)
      local want="$4" have
      have="$(unit_state "$target")"
      case "$want" in
        enabled) [[ "$have" == enabled || "$have" == static ]] || say "enable $target (now $have)" ;;
        disabled) [[ "$have" == disabled || "$have" == not-found || "$have" == static ]] || say "disable $target (now $have)" ;;
        masked) [[ "$have" == masked ]] || say "mask $target (now $have)" ;;
      esac
      ;;
    line)
      local line="$4"
      grep -qxF -- "$line" "$(path "$target")" 2>/dev/null || say "add '$line' to $target"
      ;;
    token)
      local want="$4"
      if [[ "$want" == present ]]; then
        has_token "$target" || say "add '$target' to the kernel command line"
      else
        has_token "$target" && say "remove '$target' from the kernel command line"
      fi
      ;;
  esac
  return 0
}

# --- originals ----------------------------------------------------------------

original_dir() {
  printf '%s/%s' "$ORIGINALS" "$(printf '%s' "$1" | sha256sum | cut -c1-24)"
}

# Records what the system has now, unless a record already exists: the
# first record is the state from before RackForge, and it stays.
record_original() {
  local type="$1" module="$2" id="$3" target="$4" dir temporary
  dir="$(original_dir "$id")"
  [[ -d "$dir" ]] && return 0
  mkdir -p "$ORIGINALS"
  temporary="$(mktemp -d "$ORIGINALS/.recording.XXXXXX")"
  printf '%s\n' "$id" >"$temporary/id"
  printf '%s\n' "$module" >"$temporary/module"
  printf '%s\n' "$type" >"$temporary/type"
  printf '%s\n' "$target" >"$temporary/target"
  # The order changes were made in, for reverting them newest first.
  date +%s%N >"$temporary/sequence"
  case "$type" in
    file | absent | exists)
      local file
      file="$(path "$target")"
      if [[ -e "$file" ]]; then
        cp -a "$file" "$temporary/content"
        printf 'present\n' >"$temporary/state"
      else
        printf 'absent\n' >"$temporary/state"
      fi
      ;;
    unit)
      unit_state "$target" >"$temporary/state"
      ;;
    line)
      # Only recorded when the line is about to be added, so the original
      # is always "absent".
      printf 'absent\n' >"$temporary/state"
      printf '%s\n' "$5" >"$temporary/line"
      ;;
    token)
      if has_token "$target"; then
        printf 'present\n' >"$temporary/state"
      else
        printf 'absent\n' >"$temporary/state"
      fi
      ;;
  esac
  sync -f "$temporary" 2>/dev/null || sync
  mv "$temporary" "$dir"
}

# --- applying -----------------------------------------------------------------

journal() {
  mkdir -p "$STATE_DIR"
  printf '%s %s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "$2" >>"$JOURNAL"
}

write_file() {
  local target="$1" mode="$2" source="$3" file
  file="$(path "$target")"
  mkdir -p "$(dirname "$file")"
  install -m "$mode" "$source" "$file.rackforge-new"
  mv -f "$file.rackforge-new" "$file"
  [[ "$target" == /etc/systemd/* || "$target" == /run/systemd/* ]] && SYSTEMD_CHANGED=1
  return 0
}

set_unit() {
  local unit="$1" want="$2" now="$3" have
  have="$(unit_state "$unit")"
  case "$want" in
    enabled)
      [[ "$have" == masked ]] && "$DEDICATED_SYSTEMCTL" unmask "$unit"
      "$DEDICATED_SYSTEMCTL" enable ${now:+--now} "$unit"
      ;;
    disabled)
      [[ "$have" == masked ]] && "$DEDICATED_SYSTEMCTL" unmask "$unit"
      [[ "$have" == not-found ]] || "$DEDICATED_SYSTEMCTL" disable ${now:+--now} "$unit"
      ;;
    masked)
      [[ -n "$now" ]] && { "$DEDICATED_SYSTEMCTL" stop "$unit" 2>/dev/null || true; }
      "$DEDICATED_SYSTEMCTL" mask "$unit"
      ;;
  esac
}

add_line() {
  local target="$1" line="$2" file temporary
  file="$(path "$target")"
  temporary="$file.rackforge-new"
  cp -a "$file" "$temporary"
  # config.txt applies a line to every board only below an [all] filter;
  # one is opened at the end so a line never lands inside a [pi5] section.
  if [[ "$(grep -E '^\[' "$file" | tail -1)" != "[all]" ]]; then
    printf '\n[all]\n' >>"$temporary"
  fi
  printf '%s\n' "$line" >>"$temporary"
  mv -f "$temporary" "$file"
}

remove_line() {
  local target="$1" line="$2" file temporary
  file="$(path "$target")"
  [[ -f "$file" ]] || return 0
  temporary="$file.rackforge-new"
  cp -a "$file" "$temporary"
  grep -vxF -- "$line" "$file" >"$temporary" || true
  mv -f "$temporary" "$file"
}

set_token() {
  local token="$1" want="$2" file temporary words
  file="$(cmdline_file)"
  temporary="$file.rackforge-new"
  cp -a "$file" "$temporary"
  words="$(tr -s ' \n' ' ' <"$file" | sed 's/^ //; s/ $//')"
  if [[ "$want" == present ]]; then
    words="$words $token"
  else
    words="$(tr ' ' '\n' <<<"$words" | grep -vxF -- "$token" | tr '\n' ' ' | sed 's/ $//')"
  fi
  # cmdline.txt must stay one line, ending as it ended: with a newline or,
  # as Raspberry Pi OS writes it, without, so a revert gives back the same
  # bytes.
  if [[ -n "$(tail -c 1 "$file")" ]]; then
    printf '%s' "$words" >"$temporary"
  else
    printf '%s\n' "$words" >"$temporary"
  fi
  mv -f "$temporary" "$file"
}

apply_resource() {
  local type="$1" module="$2" id="$3" target="$4"
  shift 4
  [[ -z "$(probe "$type" "$id" "$target" "$@")" ]] && return 0
  record_original "$type" "$module" "$id" "$target" "$@"
  # A unit written a moment ago is not known to systemd until it reloads.
  [[ "$type" == unit ]] && reload_systemd
  journal begin "$id"
  case "$type" in
    file) write_file "$target" "$1" "$2" ;;
    absent) rm -f "$(path "$target")" ;;
    exists)
      mkdir -p "$(dirname "$(path "$target")")"
      : >"$(path "$target")"
      ;;
    unit) set_unit "$target" "$1" "${2:-}" ;;
    line) add_line "$target" "$1" ;;
    token) set_token "$target" "$1" ;;
  esac
  journal done "$id"
  say "applied: $id"
}

# --- reverting ----------------------------------------------------------------

revert_original() {
  local dir="$1" id type target state
  id="$(cat "$dir/id")"
  type="$(cat "$dir/type")"
  target="$(cat "$dir/target")"
  state="$(cat "$dir/state")"
  journal begin "revert $id"
  case "$type" in
    file | absent | exists)
      local file
      file="$(path "$target")"
      if [[ "$state" == present ]]; then
        mkdir -p "$(dirname "$file")"
        cp -a "$dir/content" "$file.rackforge-new"
        mv -f "$file.rackforge-new" "$file"
      else
        rm -f "$file"
      fi
      [[ "$target" == /etc/systemd/* || "$target" == /run/systemd/* ]] && SYSTEMD_CHANGED=1
      ;;
    unit)
      case "$state" in
        masked) "$DEDICATED_SYSTEMCTL" mask "$target" ;;
        enabled | enabled-runtime)
          "$DEDICATED_SYSTEMCTL" unmask "$target" 2>/dev/null || true
          "$DEDICATED_SYSTEMCTL" enable "$target"
          ;;
        disabled)
          "$DEDICATED_SYSTEMCTL" unmask "$target" 2>/dev/null || true
          "$DEDICATED_SYSTEMCTL" disable "$target"
          ;;
        not-found)
          # A unit RackForge brought: it is disabled before its file goes,
          # or its enablement links would outlive it.
          "$DEDICATED_SYSTEMCTL" unmask "$target" 2>/dev/null || true
          "$DEDICATED_SYSTEMCTL" disable --now "$target" 2>/dev/null || true
          ;;
        *)
          # static, generated: nothing RackForge can set back but the mask,
          # if it set one.
          "$DEDICATED_SYSTEMCTL" unmask "$target" 2>/dev/null || true
          ;;
      esac
      ;;
    line) remove_line "$target" "$(cat "$dir/line")" ;;
    token)
      if [[ "$state" == present ]]; then
        has_token "$target" || set_token "$target" present
      else
        has_token "$target" && set_token "$target" absent
      fi
      ;;
  esac
  journal done "revert $id"
  rm -rf "$dir"
  say "reverted: $id"
}

declared_ids() {
  local record
  for record in "${RESOURCES[@]}"; do
    IFS="$US" read -r _ _ id _ <<<"$record"
    printf '%s\n' "$id"
  done
}

# Originals of what is no longer wished for -- a module dropped, a profile
# that stopped asking -- optionally only those of one module.
orphaned_originals() {
  local module="${1:-}" dir declared
  [[ -d "$ORIGINALS" ]] || return 0
  declared="$(declared_ids)"
  for dir in "$ORIGINALS"/*/; do
    [[ -f "$dir/id" ]] || continue
    [[ -n "$module" && "$(cat "$dir/module")" != "$module" ]] && continue
    grep -qxF -- "$(cat "$dir/id")" <<<"$declared" || printf '%s\n' "${dir%/}"
  done
}

all_originals() {
  local module="${1:-}" dir
  [[ -d "$ORIGINALS" ]] || return 0
  for dir in "$ORIGINALS"/*/; do
    [[ -f "$dir/id" ]] || continue
    [[ -n "$module" && "$(cat "$dir/module")" != "$module" ]] && continue
    printf '%s\n' "${dir%/}"
  done
}

# An interrupted run left a step begun and not done. Every step can be run
# again, and its original was recorded before it began, so the next run
# simply runs it again; this only says so.
report_interrupted() {
  [[ -f "$JOURNAL" ]] || return 0
  local open
  open="$(awk '{ if ($2 == "begin") { sub(/^[^ ]+ begin /, ""); open[$0] = 1 } else if ($2 == "done") { sub(/^[^ ]+ done /, ""); delete open[$0] } } END { for (id in open) print id }' "$JOURNAL")"
  if [[ -n "$open" ]]; then
    warn "a previous run stopped part-way; these steps run again:"
    printf '  %s\n' $open >&2
  fi
}

# A run that reached its end leaves nothing half-done; its journal is kept
# beside the next one's for the record.
close_journal() {
  [[ -f "$JOURNAL" ]] && mv -f "$JOURNAL" "$JOURNAL.previous"
  return 0
}

reload_systemd() {
  if [[ "$SYSTEMD_CHANGED" == 1 ]]; then
    "$DEDICATED_SYSTEMCTL" daemon-reload
    SYSTEMD_CHANGED=0
  fi
}

# --- the verbs ------------------------------------------------------------------

each_resource() {
  local callback="$1" module="${2:-}" record type rmodule id target
  local -a args
  for record in "${RESOURCES[@]}"; do
    IFS="$US" read -r -a args <<<"$record"
    type="${args[0]}"
    rmodule="${args[1]}"
    id="${args[2]}"
    target="${args[3]}"
    [[ -n "$module" && "$rmodule" != "$module" ]] && continue
    "$callback" "$type" "$rmodule" "$id" "$target" "${args[@]:4}"
  done
}

plan_one() {
  local type="$1" module="$2" id="$3" target="$4" change
  shift 4
  change="$(probe "$type" "$id" "$target" "$@")"
  [[ -n "$change" ]] && say "  [$module] $change"
  return 0
}

verify_one() {
  local type="$1" module="$2" id="$3" target="$4" change
  shift 4
  change="$(probe "$type" "$id" "$target" "$@")"
  if [[ -n "$change" ]]; then
    say "  NOT APPLIED [$module] $change"
    VERIFY_FAILED=1
  fi
  return 0
}

engine_plan() {
  local module="${1:-}" orphans
  say "plan:"
  each_resource plan_one "$module"
  orphans="$(orphaned_originals "$module")"
  if [[ -n "$orphans" ]]; then
    local dir
    for dir in $orphans; do
      say "  [$(cat "$dir/module")] revert $(cat "$dir/id") (no longer wanted)"
    done
  fi
}

engine_apply() {
  local module="${1:-}" dir
  report_interrupted
  each_resource apply_resource "$module"
  for dir in $(orphaned_originals "$module"); do
    revert_original "$dir"
  done
  reload_systemd
  close_journal
}

VERIFY_FAILED=0
engine_verify() {
  local module="${1:-}"
  VERIFY_FAILED=0
  each_resource verify_one "$module"
  return "$VERIFY_FAILED"
}

engine_revert() {
  local module="${1:-}" dirs dir
  report_interrupted
  # Newest first, so a file and the unit that reads it come back in the
  # order they went.
  dirs="$(all_originals "$module" | while read -r dir; do printf '%s %s\n' "$(cat "$dir/sequence")" "$dir"; done | sort -rn | cut -d' ' -f2-)"
  for dir in $dirs; do
    revert_original "$dir"
  done
  reload_systemd
  close_journal
}
