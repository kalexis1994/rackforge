#!/usr/bin/env bash
set -euo pipefail

# Run as an ordinary user on a disposable systemd runner. The official archive
# is never modified: all fault injection uses a separate public fixture key.
archive="$(realpath "${1:?usage: test-managed-updates.sh ARCHIVE OUTPUT}")"
output="$(realpath -m "${2:?provide a directory for test receipts}")"
repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[[ "$(id -u)" != 0 ]] || { echo 'Run update integration tests as an ordinary user with sudo.' >&2; exit 2; }
work="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rackforge-update-integration.XXXXXX")"
root="$HOME/rackforge-updates-test-linux"
mkdir -p "$work/payload" "$output"
tar -xzf "$archive" -C "$work/payload"
sudo groupadd --system --force rackforge
sudo usermod -a -G rackforge "$(id -un)"
python3 -m venv "$work/python"
python="$work/python/bin/python"
"$python" -m pip install playwright==1.65.2
if ! command -v google-chrome >/dev/null && ! command -v chromium >/dev/null; then
  "$python" -m playwright install chromium
fi
cargo build --locked --release -p rackforge-updater --features test-support
cp "$repository/target/release/rackforge-updater" "$work/test-agent"
cargo test --locked --release -p rackforge-surface-runtime --no-run --message-format=json >"$work/tests.jsonl"
little_test="$(python3 - "$work/tests.jsonl" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    item = json.loads(line)
    if item.get("reason") == "compiler-artifact" and item.get("profile", {}).get("test") and item.get("executable"):
        print(item["executable"])
PY
)"
cleanup() {
  if [[ -f "$root/.rackforge-update-test" ]]; then
    python3 "$repository/tools/test-managed-update-host.py" cleanup --root "$root"
  fi
}
trap cleanup EXIT
for interface in web little; do
  for scenario in success bad-web crash-web; do
    expected=rolled_back
    [[ "$scenario" != success ]] || expected=installed
    python3 "$repository/tools/test-managed-update-host.py" prepare --root "$root" \
      --payload "$work/payload/rackforge" --test-agent "$work/test-agent" --scenario "$scenario"
    if [[ "$interface" == web ]]; then
      "$python" "$repository/tools/test-managed-update-web.py" --expected "$expected" --receipt "$output/$interface-$scenario.json"
    else
      env RACKFORGE_UPDATE_SOCKET="/run/$(basename "$root")/updater.sock" \
        RACKFORGE_EXPECT_UPDATE_PHASE="$expected" \
        "$little_test" little_update_agent_end_to_end --ignored --nocapture >"$output/$interface-$scenario.log"
    fi
    python3 "$repository/tools/test-managed-update-host.py" check --root "$root" >"$output/$interface-$scenario-host.json"
    cleanup
  done
done
python3 "$repository/tools/test-managed-update-host.py" prepare --root "$root" \
  --payload "$work/payload/rackforge" --test-agent "$work/test-agent" --scenario bad-hash
"$python" "$repository/tools/test-managed-update-web.py" --expected failed --receipt "$output/web-bad-hash.json"
python3 "$repository/tools/test-managed-update-host.py" check --root "$root" >"$output/web-bad-hash-host.json"
