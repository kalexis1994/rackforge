#!/usr/bin/env python3
"""What the soundboard costs this machine, measured by taking it away.

Run it on the appliance, with the engine running and nothing else playing:

    python3 tools/measure-appliance-stage-split.py

Concert Grand's block time is a fixed part -- the soundboard bank, which
costs the same whether anything is being played or not -- plus a part that
grows with the notes held. Knowing the split decides what is worth doing to
make the instrument fit: spreading a fixed cost across cores buys nothing a
cheaper bank would not, and thinning voices does nothing for a machine that
cannot afford silence.

Nothing here needs a special build. Board Density is a normal parameter and
it is the only control that changes how many modes the bank has, so sweeping
it and watching the block cost move gives the per-mode price on the real
machine. The mode counts per density come from
`board_count_by_density` in the plugin's own tests -- they are deterministic
and identical on every machine, which is why they can be taken on a desktop
and used to read a sweep taken here.

What it prints, per density: the mean block cost with nothing sounding and
with a chord held. The slope of the first against the mode count is what a
board mode costs; the intercept is everything that is not the board.
"""

import json
import os
import socket
import subprocess
import sys
import time

SOCKET = os.environ.get(
    "RACKFORGE_CONTROL_SOCKET",
    os.path.expanduser("~/rackforge/state/live-control.sock"),
)
SERVICE = os.environ.get("RACKFORGE_AUDIO_UNIT", "rackforge-audio.service")
INSTANCE = os.environ.get("RACKFORGE_INSTANCE", "live.main.instrument.1")
BOARD_DENSITY = 32

# Density -> modes, from `board_count_by_density`. The bank saturates at
# BOARD_MODES, so the top of the range buys fewer modes than it looks like.
SWEEP = [
    (0.6325, 106),
    (0.9163, 153),
    (1.2000, 200),
    (1.4218, 235),
    (1.5811, 256),
]

# Enough notes to be well inside what the machine can hold, so the reading is
# a cost and not a queue of missed deadlines.
CHORD = [40, 47, 52, 59, 64, 71]


def send(payload: dict) -> dict:
    stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    stream.settimeout(10)
    stream.connect(SOCKET)
    stream.sendall((json.dumps(payload) + "\n").encode())
    buffer = b""
    while not buffer.endswith(b"\n"):
        chunk = stream.recv(65536)
        if not chunk:
            break
        buffer += chunk
    stream.close()
    return json.loads(buffer) if buffer.strip() else {}


def midi(status: int, data1: int, data2: int) -> None:
    send(
        {
            "op": "virtual_midi",
            "client_id": "measure-stage-split",
            "message": {"status": status, "data1": data1, "data2": data2},
        }
    )


def set_density(value: float) -> None:
    reply = send(
        {
            "op": "set_plugin_parameter",
            "instance_id": INSTANCE,
            "parameter_index": BOARD_DENSITY,
            "value": value,
        }
    )
    if reply.get("status") != "plugin_parameter_set":
        raise SystemExit(f"the engine refused the parameter: {reply}")


def mean_block_us(seconds: int) -> tuple[float, int]:
    """Mean block cost and deadline misses over the last `seconds`."""
    text = subprocess.run(
        ["journalctl", "-u", SERVICE, f"--since=-{seconds} sec", "--no-pager"],
        capture_output=True,
        text=True,
    ).stdout
    means: list[int] = []
    misses = 0
    for line in text.splitlines():
        if "AUDIO_RENDER_BLOCK" not in line:
            continue
        for token in line.split():
            if token.startswith("avg_us="):
                means.append(int(token.split("=", 1)[1]))
            elif token.startswith("deadline_misses="):
                misses += int(token.split("=", 1)[1])
    if not means:
        raise SystemExit("no engine telemetry; is the audio service running?")
    return sum(means) / len(means), misses


def main() -> None:
    if not os.path.exists(SOCKET):
        raise SystemExit(f"no control socket at {SOCKET}; is the engine running?")
    original = float(sys.argv[1]) if len(sys.argv) > 1 else 1.0
    print(f"{'modos':>6}  {'silencio':>12}  {'6 notas':>12}  {'por nota':>10}  fallos")
    rows = []
    try:
        for density, modes in SWEEP:
            set_density(density)
            # The bank rebuild is amortised across blocks; let it finish
            # before anything is read, or the reading is the rebuild.
            time.sleep(12)
            idle, _ = mean_block_us(10)

            midi(0xB0, 64, 127)
            for note in CHORD:
                midi(0x90, note, 100)
                time.sleep(0.05)
            time.sleep(12)
            playing, misses = mean_block_us(10)
            midi(0xB0, 123, 0)
            midi(0xB0, 64, 0)
            time.sleep(6)

            per_note = (playing - idle) / len(CHORD)
            rows.append((modes, idle, playing))
            print(
                f"{modes:>6}  {idle:>9.0f} us  {playing:>9.0f} us  "
                f"{per_note:>7.0f} us  {misses:>4d}",
                flush=True,
            )
    finally:
        set_density(original)

    if len(rows) >= 2:
        # Least squares through (modes, idle): slope is a mode, intercept is
        # everything the board is not.
        n = len(rows)
        mean_x = sum(row[0] for row in rows) / n
        mean_y = sum(row[1] for row in rows) / n
        covariance = sum((row[0] - mean_x) * (row[1] - mean_y) for row in rows)
        variance = sum((row[0] - mean_x) ** 2 for row in rows)
        slope = covariance / variance if variance else 0.0
        intercept = mean_y - slope * mean_x
        full = slope * 256
        print(
            f"\nun modo del tablero: {slope * 1000:.1f} ns"
            f"\nbanco completo (256 modos): {full:.0f} us"
            f"\ntodo lo demas en silencio: {intercept:.0f} us"
            f"\nel tablero es el {full / (full + intercept) * 100:.0f} % del silencio"
        )


if __name__ == "__main__":
    main()
