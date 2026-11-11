"""Finds clicks in a RackForge output capture and names the MIDI beside each.

    python tools/find-clicks.py click-1758600000.wav [--threshold 8]

The desktop's flight recorder (the dot beside the OUT meter) saves the last
fifteen seconds of exactly what went to the audio device as `<stem>.wav`, and
the MIDI that played them as `<stem>.txt`. This reads both.

A click is a discontinuity: a jump from one sample to the next that the
signal's own motion does not explain. The second difference -- how much the
slope changes from one sample to the next -- is small for anything a string,
a tine or a filter produces, because they move smoothly, and large exactly at
a step or a corner. Each sample's second difference is compared with the
typical second difference around it, so a loud passage does not read as a
click and a quiet one does not hide one. What stands out by more than
`threshold` times its surroundings is reported, with the MIDI messages that
landed within `window` milliseconds of it.

A capture with clicks here means the click is in the audio RackForge
produced, and the messages beside it are the suspects. A capture that is
clean where a click was heard means it was added after RackForge: the
driver, the USB link or the interface.
"""

import argparse
import pathlib
import re
import sys

import numpy as np
from scipy.io import wavfile
from scipy.ndimage import median_filter


def load_events(listing: pathlib.Path):
    events = []
    if not listing.exists():
        return events
    for line in listing.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        match = re.match(r"\s*([\d.]+)\s+(\d+)\s+((?:[0-9A-F]{2}\s?)+)\s+(.*)$", line)
        if match:
            events.append((float(match.group(1)), match.group(4).strip()))
    return events


def find_clicks(samples: np.ndarray, rate: int, threshold: float, neighbourhood_ms: float):
    """Frames whose second difference stands out from its neighbourhood."""
    mono = samples.mean(axis=1) if samples.ndim == 2 else samples
    curvature = np.abs(np.diff(mono, n=2))
    width = max(3, int(rate * neighbourhood_ms / 1000) | 1)
    # The median ignores the click itself, which a mean would not.
    typical = median_filter(curvature, size=width, mode="nearest")
    floor = max(float(np.percentile(curvature, 50)), 1e-7)
    ratio = curvature / np.maximum(typical, floor)
    candidates = np.flatnonzero(ratio > threshold)
    clicks = []
    for frame in candidates:
        # One click spans a few samples; report its strongest one.
        if clicks and frame - clicks[-1][0] < rate // 200:
            if ratio[frame] > clicks[-1][1]:
                clicks[-1] = (frame + 1, ratio[frame], curvature[frame])
            continue
        clicks.append((frame + 1, ratio[frame], curvature[frame]))
    return clicks


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("wav", type=pathlib.Path)
    parser.add_argument("--threshold", type=float, default=8.0,
                        help="how many times its surroundings a jump must be (default 8)")
    parser.add_argument("--window", type=float, default=15.0,
                        help="milliseconds around a click to list MIDI from (default 15)")
    parser.add_argument("--neighbourhood", type=float, default=5.0,
                        help="milliseconds of surroundings a jump is compared with (default 5)")
    arguments = parser.parse_args()

    rate, samples = wavfile.read(arguments.wav)
    samples = samples.astype(np.float64)
    events = load_events(arguments.wav.with_suffix(".txt"))
    clicks = find_clicks(samples, rate, arguments.threshold, arguments.neighbourhood)

    duration = len(samples) / rate
    print(f"{arguments.wav.name}: {duration:.2f} s at {rate} Hz, "
          f"peak {np.abs(samples).max():.3f}, {len(events)} MIDI messages")
    if not clicks:
        print(f"no discontinuity above {arguments.threshold}x its surroundings.")
        print("If a click was heard inside this stretch, it was added after RackForge.")
        return 0
    print(f"{len(clicks)} discontinuities above {arguments.threshold}x their surroundings:\n")
    window = arguments.window / 1000.0
    for frame, ratio, jump in clicks:
        at = frame / rate
        level = np.abs(samples[max(0, frame - rate // 100): frame + rate // 100]).max()
        print(f"  {at:8.4f} s  frame {frame:>8}  {ratio:7.1f}x  jump {jump:.5f}  level {level:.3f}")
        for time, meaning in events:
            if abs(time - at) <= window:
                print(f"              {1000 * (time - at):+7.1f} ms  {meaning}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
