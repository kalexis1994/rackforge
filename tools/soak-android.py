#!/usr/bin/env python3
"""Run a configurable RackForge Android native-audio soak qualification."""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LIFECYCLE_SCRIPT = Path(__file__).with_name("qualify-android-lifecycle.py")
SPEC = importlib.util.spec_from_file_location("rackforge_android_adb", LIFECYCLE_SCRIPT)
ADB_SUPPORT = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(ADB_SUPPORT)

QUALIFIED_DURATION_SECONDS = 2 * 60 * 60
MIDI_PULSE_ACTION = "org.rackforge.android.action.QUALIFICATION_MIDI_PULSE"

COUNTER_PATHS = {
    "xruns": ("audio_status", "xruns"),
    "queue_underruns": ("audio_status", "render_queue_underruns"),
    "missed_deadlines": ("audio_status", "callback_overruns"),
    "midi_dropped": ("audio_status", "midi_dropped_events"),
    "render_errors": ("audio_status", "render_errors"),
    "engine_lock_misses": ("audio_status", "engine_lock_misses"),
    "stream_losses": ("audio_status", "stream_losses"),
    "stream_recoveries": ("audio_status", "stream_recoveries"),
    "nonfinite_samples": ("audio_status", "nonfinite_samples"),
    "midi_disconnect_panics": ("audio_status", "midi_panic_count"),
    "midi_reconnect_attempts": ("midi_reconnect_attempts",),
}

DEFAULT_LIMITS = {
    "xruns": 0,
    "queue_underruns": 0,
    "missed_deadlines": 0,
    "midi_dropped": 0,
    "render_errors": 0,
    "engine_lock_misses": 0,
    "stream_losses": 0,
    "stream_recoveries": 0,
    "nonfinite_samples": 0,
    "midi_disconnect_panics": 0,
    "callback_stalls": 0,
    "process_restarts": 0,
    "snapshot_failures": 0,
    "midi_pulse_failures": 0,
    "unhealthy_samples": 0,
}


def nested_integer(value: dict[str, Any], path: tuple[str, ...]) -> int:
    current: Any = value
    for key in path:
        if not isinstance(current, dict):
            return 0
        current = current.get(key, 0)
    try:
        return max(0, int(current))
    except (TypeError, ValueError):
        return 0


def reset_aware_delta(previous: int, current: int) -> int:
    return current - previous if current >= previous else current


def total_pss_kib(adb: Any, package: str) -> int | None:
    output = adb.shell("dumpsys", "meminfo", package, timeout=60.0)
    for pattern in (r"TOTAL PSS:\s+(\d+)", r"^\s*TOTAL\s+(\d+)"):
        match = re.search(pattern, output, re.MULTILINE)
        if match:
            return int(match.group(1))
    return None


def send_midi_pulse(adb: Any, package: str, note: int) -> None:
    adb.shell(
        "am",
        "broadcast",
        "--receiver-foreground",
        "-a",
        MIDI_PULSE_ACTION,
        "-n",
        f"{package}/.QualificationReceiver",
        "--ei",
        "note",
        str(note),
        "--ei",
        "velocity",
        "96",
        "--el",
        "duration_ms",
        "300",
    )


def evaluate(
    totals: dict[str, int],
    maxima: dict[str, float],
    duration_seconds: float,
    limits: dict[str, int],
    maximum_callback_load: float,
    maximum_thermal_status: int,
) -> tuple[str, list[str]]:
    failures = [
        f"{metric}={totals.get(metric, 0)} exceeds {limit}"
        for metric, limit in limits.items()
        if totals.get(metric, 0) > limit
    ]
    if maxima.get("callback_load_percent", 0.0) > maximum_callback_load:
        failures.append(
            "callback_load_percent="
            f"{maxima['callback_load_percent']:.2f} exceeds {maximum_callback_load:.2f}"
        )
    if maxima.get("thermal_status", 0.0) > maximum_thermal_status:
        failures.append(
            f"thermal_status={int(maxima['thermal_status'])} exceeds {maximum_thermal_status}"
        )
    if failures:
        return "failed", failures
    if duration_seconds < QUALIFIED_DURATION_SECONDS:
        return "passed_with_duration_waiver", []
    return "passed", []


def compact_sample(snapshot: dict[str, Any], elapsed: float, pid: str, pss: int | None) -> dict[str, Any]:
    status = snapshot.get("audio_status", {})
    return {
        "elapsed_seconds": round(elapsed, 3),
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "pid": pid,
        "total_pss_kib": pss,
        "thermal_status": snapshot.get("thermal_status", 0),
        "midi_reconnect_attempts": snapshot.get("midi_reconnect_attempts", 0),
        "open_midi_ports": snapshot.get("open_midi_ports", 0),
        "audio_status": {
            key: status.get(key)
            for key in (
                "running",
                "device_id",
                "xruns",
                "callback_count",
                "callback_overruns",
                "callback_load_percent",
                "maximum_callback_us",
                "callback_budget_us",
                "render_queue_underruns",
                "midi_dropped_events",
                "engine_lock_misses",
                "render_errors",
                "stream_health",
                "stream_losses",
                "stream_recoveries",
                "nonfinite_samples",
            )
        },
    }


def run(arguments: argparse.Namespace) -> dict[str, Any]:
    adb_path = ADB_SUPPORT.locate_adb(arguments.adb)
    serial = ADB_SUPPORT.choose_serial(adb_path, arguments.serial)
    adb = ADB_SUPPORT.Adb(adb_path, serial)
    package = arguments.package
    adb.shell("am", "start", "-W", "-n", f"{package}/.MainActivity", timeout=60.0)
    baseline = ADB_SUPPORT.wait_for_snapshot(
        adb,
        package,
        "native audio startup",
        lambda value: bool(value.get("audio_running"))
        and bool(value.get("audio_status", {}).get("running")),
        arguments.startup_timeout,
    )
    duration_seconds = arguments.duration_minutes * 60.0
    started_at = datetime.now(timezone.utc).isoformat()
    started = time.monotonic()
    deadline = started + duration_seconds
    next_sample = started
    next_pulse = started
    note_index = 0
    notes = (48, 55, 60, 64, 67, 72)
    previous = {name: nested_integer(baseline, path) for name, path in COUNTER_PATHS.items()}
    previous_callback = nested_integer(baseline, ("audio_status", "callback_count"))
    baseline_pid = adb.shell("pidof", package)
    previous_pid = baseline_pid
    totals = {name: 0 for name in COUNTER_PATHS}
    totals.update(
        {
            "callback_stalls": 0,
            "process_restarts": 0,
            "snapshot_failures": 0,
            "midi_pulse_failures": 0,
            "unhealthy_samples": 0,
        }
    )
    maxima = {
        "callback_load_percent": 0.0,
        "maximum_callback_us": 0.0,
        "thermal_status": float(baseline.get("thermal_status", 0)),
        "total_pss_kib": 0.0,
    }
    samples: list[dict[str, Any]] = []

    print(
        f"Soaking {arguments.duration_minutes:g} minutes on {serial}; "
        f"sampling every {arguments.sample_seconds:g}s"
    )
    while time.monotonic() < deadline:
        now = time.monotonic()
        if arguments.midi_pulse_seconds > 0 and now >= next_pulse:
            try:
                send_midi_pulse(adb, package, notes[note_index % len(notes)])
            except (ADB_SUPPORT.QualificationError, subprocess.TimeoutExpired):
                totals["midi_pulse_failures"] += 1
            note_index += 1
            next_pulse = now + arguments.midi_pulse_seconds
        if now < next_sample:
            time.sleep(min(0.25, next_sample - now, max(0.0, deadline - now)))
            continue
        try:
            snapshot = ADB_SUPPORT.get_snapshot(adb, package)
            pid = adb.shell("pidof", package)
            pss = total_pss_kib(adb, package)
        except (ADB_SUPPORT.QualificationError, subprocess.TimeoutExpired) as error:
            totals["snapshot_failures"] += 1
            samples.append(
                {
                    "elapsed_seconds": round(now - started, 3),
                    "timestamp": datetime.now(timezone.utc).isoformat(),
                    "error": str(error),
                }
            )
            next_sample = now + arguments.sample_seconds
            continue

        for name, path in COUNTER_PATHS.items():
            current = nested_integer(snapshot, path)
            totals[name] += reset_aware_delta(previous[name], current)
            previous[name] = current
        callback = nested_integer(snapshot, ("audio_status", "callback_count"))
        if callback == previous_callback:
            totals["callback_stalls"] += 1
        previous_callback = callback
        if pid != previous_pid:
            totals["process_restarts"] += 1
            previous_pid = pid
        status = snapshot.get("audio_status", {})
        if not snapshot.get("audio_running") or status.get("stream_health") != "healthy":
            totals["unhealthy_samples"] += 1
        maxima["callback_load_percent"] = max(
            maxima["callback_load_percent"], float(status.get("callback_load_percent", 0.0) or 0.0)
        )
        maxima["maximum_callback_us"] = max(
            maxima["maximum_callback_us"], float(status.get("maximum_callback_us", 0.0) or 0.0)
        )
        maxima["thermal_status"] = max(
            maxima["thermal_status"], float(snapshot.get("thermal_status", 0) or 0)
        )
        if pss is not None:
            maxima["total_pss_kib"] = max(maxima["total_pss_kib"], float(pss))
        samples.append(compact_sample(snapshot, now - started, pid, pss))
        print(
            f"{(now - started) / 60:7.2f} min · callbacks {callback} · "
            f"xruns {totals['xruns']} · deadlines {totals['missed_deadlines']} · "
            f"MIDI dropped {totals['midi_dropped']}"
        )
        next_sample = now + arguments.sample_seconds

    actual_duration = time.monotonic() - started
    outcome, failures = evaluate(
        totals,
        maxima,
        actual_duration,
        DEFAULT_LIMITS,
        arguments.maximum_callback_load,
        arguments.maximum_thermal_status,
    )
    return {
        "schema_version": 1,
        "kind": "rackforge.android.native-soak",
        "started_at": started_at,
        "outcome": outcome,
        "failures": failures,
        "qualified_duration_seconds": QUALIFIED_DURATION_SECONDS,
        "requested_duration_seconds": duration_seconds,
        "actual_duration_seconds": actual_duration,
        "sample_interval_seconds": arguments.sample_seconds,
        "midi_pulse_interval_seconds": arguments.midi_pulse_seconds,
        "hardware": ADB_SUPPORT.hardware_metadata(adb),
        "runtime_environment": {
            "rackforge_version": baseline.get("version"),
            "rackforge_revision": baseline.get("revision"),
            "platform": baseline.get("platform"),
            "selected_audio_output": baseline.get("selected_audio_output"),
            "selected_audio_device_id": baseline.get("selected_audio_device_id"),
            "audio_outputs": baseline.get("audio_outputs", []),
            "midi_devices": baseline.get("midi_devices", []),
            "usb_devices": baseline.get("usb_devices", []),
        },
        "adb_serial": serial,
        "package": package,
        "limits": {
            **DEFAULT_LIMITS,
            "maximum_callback_load_percent": arguments.maximum_callback_load,
            "maximum_thermal_status": arguments.maximum_thermal_status,
        },
        "totals": totals,
        "maxima": maxima,
        "baseline": compact_sample(baseline, 0.0, baseline_pid, None),
        "samples": samples,
        "finished_at": datetime.now(timezone.utc).isoformat(),
    }


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb")
    parser.add_argument("--serial")
    parser.add_argument("--package", default=ADB_SUPPORT.DEFAULT_PACKAGE)
    parser.add_argument("--duration-minutes", type=float, default=120.0)
    parser.add_argument("--sample-seconds", type=float, default=10.0)
    parser.add_argument("--midi-pulse-seconds", type=float, default=2.0)
    parser.add_argument("--startup-timeout", type=float, default=90.0)
    parser.add_argument("--maximum-callback-load", type=float, default=85.0)
    parser.add_argument("--maximum-thermal-status", type=int, default=2)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    if arguments.duration_minutes <= 0 or arguments.sample_seconds <= 0:
        parser.error("duration and sample interval must be greater than zero")
    return arguments


def main() -> int:
    arguments = parse_arguments()
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = arguments.output or Path(f"dist/qualification/android-soak-{timestamp}.json")
    try:
        report = run(arguments)
    except (ADB_SUPPORT.QualificationError, subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        report = {
            "schema_version": 1,
            "kind": "rackforge.android.native-soak",
            "outcome": "failed",
            "failures": [str(error)],
            "finished_at": datetime.now(timezone.utc).isoformat(),
        }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Report: {output.resolve()}")
    if report["outcome"] == "failed":
        for failure in report.get("failures", []):
            print(f"FAIL  {failure}", file=sys.stderr)
        return 1
    if report["outcome"] == "passed":
        print("PASS  Two-hour native Android soak qualification")
    else:
        print("PASS  Short soak smoke test (duration waiver recorded)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
