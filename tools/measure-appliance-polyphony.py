#!/usr/bin/env python3
"""How many notes this machine can actually hold, measured on the machine.

Run it on the appliance, with the engine running and nothing else playing:

    python3 tools/measure-appliance-polyphony.py

It presses notes through Core's own control socket rather than through the
Web interface, so it works with the interface stopped and it plays the same
notes every time. For each step it holds the pedal, adds notes, waits, and
reads what the engine reported while they rang.

The number that matters is the step where misses stop being zero. That is the
polyphony this instrument has on this machine at this period size, and it is a
property of the three together -- a faster machine, a cheaper instrument or a
longer period each move it.

## It used to report zero misses through a ramp that missed 571 (2026-09-19)

This tool held the notes for twelve seconds and then read the last TEN, so
that it would describe "the part of the notes that lasts" rather than the
strike. That is a reasonable thing to want for a mean. It is the wrong window
for a MISS, because a miss is a rare event and the rare events were in the two
seconds it cut off -- and, far worse, in the budget governor's rebuilds, which
land wherever they land.

Measured against the engine's own journal over one ramp: this tool printed
`misses 0` at every step from two notes to sixteen, while `AUDIO_RENDER_BLOCK`
logged **571** deadline misses over the same period, including one window with
304 misses out of 345 blocks and a mean block cost of 2874 us against a 2666 us
deadline. The budget governor, counting independently, saw the same lateness
(17, 216, 302). Two counters agreed and the tool disagreed with both.

So the window now brackets the whole hold, from the moment the first note goes
down to the moment before the notes are lifted, and misses are totalled over
all of it. The old question -- what does the sustained part cost -- is still
answered, in its own column, where it cannot hide a miss.

It also prints what the governor did during the step. The misses cluster
around its cuts: a cut rebuilds the banks, and a rebuild is expensive. The
forty-millisecond fade makes that inaudible, not cheap. A step that missed
while the budget moved is a different fact from a step that missed under load,
and reading the first as the second is how a governor that is causing xruns
looks like an instrument that is too expensive.
"""

import json
import math
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
# 128 frames at 48 kHz. Read from the engine rather than assumed, below.
DEFAULT_DEADLINE_US = 128 / 48_000 * 1e6
# How long the notes are held, and how much of the front of that is counted
# as the strike rather than the sustain. Both are reported; neither is hidden.
HOLD_S = int(os.environ.get("RACKFORGE_HOLD_S", "12"))
ATTACK_S = int(os.environ.get("RACKFORGE_ATTACK_S", "2"))
# Long enough for the dampers to land and the previous step to leave the
# window. Six seconds used to let a step's tail into the next step's mean.
GAP_S = int(os.environ.get("RACKFORGE_GAP_S", "10"))


def send(payload: dict) -> None:
    stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    stream.settimeout(5)
    stream.connect(SOCKET)
    stream.sendall((json.dumps(payload) + "\n").encode())
    stream.recv(4096)
    stream.close()


def midi(status: int, data1: int, data2: int) -> None:
    send(
        {
            "op": "virtual_midi",
            "client_id": "measure-polyphony",
            "message": {"status": status, "data1": data1, "data2": data2},
        }
    )


def journal(seconds: int) -> str:
    """The last `seconds` of the unit's log.

    Relative, never an absolute timestamp: journalctl reads `--since` in local
    time, `datetime.utcnow()` is not local, and the mismatch returns an empty
    log silently rather than an error -- which reads exactly like a window
    with nothing wrong in it.
    """
    return subprocess.run(
        ["journalctl", "-u", SERVICE, f"--since=-{max(seconds, 1)}sec", "--no-pager"],
        capture_output=True,
        text=True,
    ).stdout


def field(line: str, name: str) -> int | None:
    for token in line.split():
        if token.startswith(f"{name}="):
            try:
                return int(token.split("=", 1)[1])
            except ValueError:
                return None
    return None


def deadline_us(text: str) -> float:
    for line in text.splitlines():
        if "AUDIO_RENDER_BLOCK" in line:
            value = field(line, "deadline_us")
            if value:
                return float(value)
    return DEFAULT_DEADLINE_US


def measure(text: str) -> dict | None:
    """What the engine reported, over whatever window `text` covers."""
    blocks = [line for line in text.splitlines() if "AUDIO_RENDER_BLOCK" in line]
    if not blocks:
        return None
    means = [field(line, "avg_us") or 0 for line in blocks]
    return {
        "windows": len(blocks),
        "mean": sum(means) / len(means),
        "worst": max(field(line, "p99_us") or 0 for line in blocks),
        "peak": max(field(line, "max_us") or 0 for line in blocks),
        "misses": sum(field(line, "deadline_misses") or 0 for line in blocks),
        "deadline": deadline_us(text),
    }


def budget_moves(text: str) -> list[str]:
    """What the governor did in the window, since its cuts rebuild the banks."""
    moves = []
    for line in text.splitlines():
        if "AUDIO_QUALITY_BUDGET" not in line:
            continue
        reason = next(
            (t.split("=", 1)[1] for t in line.split() if t.startswith("reason=")), "?"
        )
        if reason in ("seeded", "settled"):
            continue
        fuel = field(line, "fuel")
        late = field(line, "late")
        blocks = field(line, "blocks")
        moves.append(f"{reason} a {fuel} ({late}/{blocks} tarde)")
    return moves


def main() -> None:
    steps = [int(value) for value in sys.argv[1:]] or [2, 4, 6, 8, 12, 16]
    if not os.path.exists(SOCKET):
        raise SystemExit(f"no control socket at {SOCKET}; is the engine running?")
    print("             ---- todo el sostenido ----   -- solo lo sostenido --")
    print(
        f"{'notas':>6} {'media':>9} {'p99':>7} {'pico':>7} {'faltas':>7}"
        f"{'media':>10} {'p99':>7} {'faltas':>7}",
        flush=True,
    )
    for held in steps:
        started = time.monotonic()
        midi(0xB0, 64, 127)  # sustain down, so the notes pile up rather than stop
        for index in range(held):
            midi(0x90, 40 + index * 4, 100)
            time.sleep(0.05)
        time.sleep(HOLD_S)
        # The whole hold, from the first note down to now -- rounded up, plus
        # one second, so no block of it falls outside the window.
        text = journal(math.ceil(time.monotonic() - started) + 1)
        whole = measure(text)
        # And the same hold with the strike cut off, which is what this tool
        # used to report on its own.
        rest = measure(journal(max(HOLD_S - ATTACK_S, 1)))
        if whole is None:
            print(f"{held:6d}  sin telemetria (¿esta corriendo el motor?)", flush=True)
        else:
            share = whole["mean"] / whole["deadline"] * 100
            line = (
                f"{held:6d} {whole['mean']:7.0f}us {share:5.0f}% "
                f"{whole['worst']:5d}us {whole['peak']:5d}us {whole['misses']:6d}"
            )
            if rest is not None:
                line += f"  {rest['mean']:7.0f}us {rest['worst']:5d}us {rest['misses']:6d}"
            print(line, flush=True)
        for move in budget_moves(text):
            print(f"{'':6}   ^ el gobernador: {move}", flush=True)
        midi(0xB0, 123, 0)  # all notes off
        midi(0xB0, 64, 0)
        time.sleep(GAP_S)


if __name__ == "__main__":
    main()
