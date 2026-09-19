#!/usr/bin/env python3
"""How many notes this machine can actually hold, measured on the machine.

Run it on the appliance, with the engine running and nothing else playing:

    python3 tools/measure-appliance-polyphony.py

It presses notes through Core's own control socket rather than through the
Web interface, so it works with the interface stopped and it plays the same
notes every time. For each step it holds the pedal, adds notes, waits for the
attacks to pass, and reads what the engine reported while they rang.

What it prints, per step: the mean block cost as a share of the deadline, the
worst p99 the engine saw, and the deadline misses. Misses are xruns: a block
that arrived late is a gap in the audio, and the number is per ten seconds.

The number that matters is the step where misses stop being zero. That is the
polyphony this instrument has on this machine at this period size, and it is a
property of the three together -- a faster machine, a cheaper instrument or a
longer period each move it.
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
# 128 frames at 48 kHz. Read from the engine rather than assumed, below.
DEFAULT_DEADLINE_US = 128 / 48_000 * 1e6


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
    return subprocess.run(
        ["journalctl", "-u", SERVICE, f"--since=-{seconds} sec", "--no-pager"],
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


def report(seconds: int) -> str:
    text = journal(seconds)
    blocks = [line for line in text.splitlines() if "AUDIO_RENDER_BLOCK" in line]
    if not blocks:
        return "no engine telemetry in the window (is the audio service running?)"
    deadline = deadline_us(text)
    means = [field(line, "avg_us") or 0 for line in blocks]
    worst = max(field(line, "p99_us") or 0 for line in blocks)
    misses = sum(field(line, "deadline_misses") or 0 for line in blocks)
    mean = sum(means) / len(means)
    return (
        f"mean {mean:5.0f} us ({mean / deadline * 100:4.0f}% of the deadline) | "
        f"worst p99 {worst:5d} us | misses {misses:4d}"
    )


def main() -> None:
    steps = [int(value) for value in sys.argv[1:]] or [2, 4, 6, 8, 12, 16]
    if not os.path.exists(SOCKET):
        raise SystemExit(f"no control socket at {SOCKET}; is the engine running?")
    for held in steps:
        midi(0xB0, 64, 127)  # sustain down, so the notes pile up rather than stop
        for index in range(held):
            midi(0x90, 40 + index * 4, 100)
            time.sleep(0.05)
        # Past the attacks, into the part of the notes that lasts.
        time.sleep(12)
        print(f"{held:3d} notes held: {report(10)}", flush=True)
        midi(0xB0, 123, 0)  # all notes off
        midi(0xB0, 64, 0)
        time.sleep(6)


if __name__ == "__main__":
    main()
