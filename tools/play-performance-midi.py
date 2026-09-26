#!/usr/bin/env python3
"""Play a real performance into the running engine, and count what it misses.

    python3 tools/play-performance-midi.py FILE.midi [--from MIN] [--minutes N]

Run it on the appliance, with the engine running and nothing else playing.

## Why this exists

`measure-appliance-polyphony.py` presses N keys and holds them. Measured
against MAESTRO -- real competition performances captured on Disklaviers,
with all three pedals -- that is not what a pianist does:

    piece                       voices ringing   attacks in one 2.7 ms block
    Liszt, La Campanella                    47                             5
    Chopin, Nocturne Op. 27/2               22                             3
    a dense 8-minute programme              36                             7

So the ramp overstated the transient (twelve attacks in one block, where
the worst real moment is seven) and understated the steady load (twelve
voices, where the pedal leaves forty-seven ringing). A performance is the
honest test of both at once, and a deadline missed inside one is a gap the
player would actually have heard.

## What it reports

Deadline misses over the whole piece, from the engine's own
`AUDIO_RENDER_BLOCK` telemetry, plus where they happened -- so a miss is a
bar number rather than a number. It also reports its OWN scheduling error,
because a player that cannot keep time is measuring itself: if the late
sends are more than a millisecond the result says nothing about the
engine.
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from importlib import import_module

analyse = import_module("analyse-performance-midi")

SOCKET = os.environ.get(
    "RACKFORGE_CONTROL_SOCKET",
    os.path.expanduser("~/rackforge/state/live-control.sock"),
)
SERVICE = os.environ.get("RACKFORGE_AUDIO_UNIT", "rackforge-audio.service")


class Control:
    """A connection per message, because the engine closes after each one.

    Keeping one open was tried first and the second send raises a broken
    pipe. So a connect, a send and a reply sit on the critical path of every
    note, and `cost_us` reports what that costs: at a dense moment seven
    notes land inside one 2.7 ms block, and if the socket takes more than a
    fraction of a millisecond then the player is what arrives late and the
    measurement is of the player.
    """

    def __init__(self, path: str) -> None:
        self.path = path
        self.costs: list[float] = []

    def midi(self, status: int, one: int, two: int) -> None:
        payload = {
            "op": "virtual_midi",
            "client_id": "play-performance",
            "message": {"status": status, "data1": one, "data2": two},
        }
        started = time.perf_counter()
        stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        stream.settimeout(5)
        try:
            stream.connect(self.path)
            stream.sendall((json.dumps(payload) + "\n").encode())
            stream.recv(4096)
        finally:
            stream.close()
        self.costs.append(time.perf_counter() - started)

    def drain(self) -> None:
        """Nothing to drain: every connection is closed where it was made."""

    def cost_us(self) -> tuple[float, float]:
        if not self.costs:
            return 0.0, 0.0
        ordered = sorted(self.costs)
        return (
            sum(ordered) / len(ordered) * 1e6,
            ordered[min(int(len(ordered) * 0.99), len(ordered) - 1)] * 1e6,
        )

    def close(self) -> None:
        """Nothing to close."""


def journal(seconds: int) -> str:
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


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("midi")
    parser.add_argument("--from", dest="start", type=float, default=0.0,
                        help="skip to this many minutes in")
    parser.add_argument("--minutes", type=float, default=0.0,
                        help="stop after this many minutes (0 = the whole piece)")
    arguments = parser.parse_args()

    events, division = analyse.read_events(Path(arguments.midi))
    played = analyse.timeline(events, division)
    begin = arguments.start * 60.0
    end = begin + arguments.minutes * 60.0 if arguments.minutes else float("inf")
    played = [event for event in played if begin <= event[0] <= end]
    if not played:
        raise SystemExit("nothing to play in that range")
    origin = played[0][0]

    if not os.path.exists(SOCKET):
        raise SystemExit(f"no control socket at {SOCKET}; is the engine running?")
    control = Control(SOCKET)
    print(
        f"{Path(arguments.midi).name}: {len(played)} eventos, "
        f"{(played[-1][0] - origin) / 60:.1f} min",
        flush=True,
    )

    # Everything off first, so a previous run cannot leak into this one.
    control.midi(0xB0, 123, 0)
    control.midi(0xB0, 64, 0)
    time.sleep(0.5)
    control.drain()

    started = time.perf_counter()
    worst_late = 0.0
    late_sends = 0
    for index, (seconds, status, one, two) in enumerate(played):
        due = started + (seconds - origin)
        now = time.perf_counter()
        if due > now:
            time.sleep(due - now)
        else:
            behind = now - due
            worst_late = max(worst_late, behind)
            if behind > 0.001:
                late_sends += 1
        control.midi(status, one, two)
        if index % 256 == 0:
            control.drain()
    # Let the tail ring, then lift everything.
    time.sleep(3.0)
    control.midi(0xB0, 123, 0)
    control.midi(0xB0, 64, 0)
    control.drain()
    control.close()
    elapsed = time.perf_counter() - started

    print(
        f"  el reproductor: peor retraso {worst_late * 1000:.1f} ms, "
        f"{late_sends} envios con mas de 1 ms de retraso de {len(played)}",
        flush=True,
    )
    mean_us, p99_us = control.cost_us()
    print(f"    el socket de control cuesta {mean_us:.0f} us de media, {p99_us:.0f} us p99")
    if worst_late > 0.010:
        print("  OJO: el reproductor no mantuvo el tiempo; el resultado es suyo, no del motor.")

    text = journal(int(elapsed) + 5)
    blocks = [line for line in text.splitlines() if "AUDIO_RENDER_BLOCK" in line]
    if not blocks:
        raise SystemExit("no engine telemetry for the window")
    misses = sum(field(line, "deadline_misses") or 0 for line in blocks)
    worst_p99 = max(field(line, "p99_us") or 0 for line in blocks)
    peak = max(field(line, "max_us") or 0 for line in blocks)
    means = [field(line, "avg_us") or 0 for line in blocks]
    deadline = next(
        (field(line, "deadline_us") for line in blocks if field(line, "deadline_us")),
        2666,
    )
    print(f"  el motor: {len(blocks)} ventanas de un segundo")
    print(f"    media {sum(means) / len(means):.0f} us ({sum(means) / len(means) / deadline * 100:.0f} % del deadline)")
    print(f"    peor p99 {worst_p99} us, pico {peak} us, deadline {deadline} us")
    print(f"    FALTAS DE DEADLINE: {misses}")
    if misses:
        print("    cuando:")
        for offset, line in enumerate(blocks):
            count = field(line, "deadline_misses") or 0
            if count:
                at = offset  # one window per second
                print(f"      {at // 60}:{at % 60:02d}  {count} faltas  "
                      f"(avg {field(line, 'avg_us')} us, max {field(line, 'max_us')} us)")
    budget = [line for line in text.splitlines() if "AUDIO_QUALITY_BUDGET" in line]
    moved = [line for line in budget if "reason=tightened" in line]
    print(f"  el gobernador: {len(moved)} recortes")
    for line in moved:
        print(f"    fuel={field(line, 'fuel')} late={field(line, 'late')}/{field(line, 'blocks')}")


if __name__ == "__main__":
    main()
