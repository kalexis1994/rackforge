#!/usr/bin/env python3
"""Qualify RackForge Android lifecycle and optional USB hotplug over ADB."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


DEFAULT_PACKAGE = "org.rackforge.android"
SNAPSHOT_ACTION = "org.rackforge.android.action.QUALIFICATION_SNAPSHOT"
SNAPSHOT_FILE = "files/rackforge-qualification.json"


class QualificationError(RuntimeError):
    """A device or RackForge invariant failed during qualification."""


class Adb:
    def __init__(self, executable: str, serial: str):
        self.executable = executable
        self.serial = serial

    def run(self, *arguments: str, timeout: float = 30.0) -> str:
        command = [self.executable, "-s", self.serial, *arguments]
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise QualificationError(f"ADB command failed: {' '.join(command)}\n{detail}")
        return result.stdout.strip()

    def shell(self, *arguments: str, timeout: float = 30.0) -> str:
        return self.run("shell", *arguments, timeout=timeout)


def locate_adb(explicit: str | None) -> str:
    candidates = [explicit, shutil.which("adb")]
    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        candidates.append(
            str(Path(local_app_data) / "RackForge/android-sdk/platform-tools/adb.exe")
        )
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return str(Path(candidate).resolve())
    raise QualificationError("adb was not found; pass --adb or install Android platform-tools")


def connected_serials(adb: str) -> list[str]:
    result = subprocess.run(
        [adb, "devices"], capture_output=True, text=True, encoding="utf-8", check=False
    )
    if result.returncode != 0:
        raise QualificationError(result.stderr.strip() or "adb devices failed")
    return [
        line.split()[0]
        for line in result.stdout.splitlines()[1:]
        if len(line.split()) >= 2 and line.split()[1] == "device"
    ]


def choose_serial(adb: str, requested: str | None) -> str:
    serials = connected_serials(adb)
    if requested:
        if requested not in serials:
            raise QualificationError(f"ADB device {requested!r} is not connected and authorized")
        return requested
    if len(serials) != 1:
        raise QualificationError(
            f"expected exactly one authorized ADB device, found {len(serials)}; pass --serial"
        )
    return serials[0]


def get_snapshot(adb: Adb, package: str) -> dict[str, Any]:
    component = f"{package}/.QualificationReceiver"
    adb.shell(
        "am",
        "broadcast",
        "--receiver-foreground",
        "-a",
        SNAPSHOT_ACTION,
        "-n",
        component,
    )
    raw = adb.run("exec-out", "run-as", package, "cat", SNAPSHOT_FILE)
    try:
        snapshot = json.loads(raw)
    except json.JSONDecodeError as error:
        raise QualificationError(f"qualification snapshot is not valid JSON: {error}") from error
    if not snapshot.get("ready"):
        raise QualificationError(snapshot.get("error", "RackForge snapshot is not ready"))
    return snapshot


def wait_for_snapshot(
    adb: Adb,
    package: str,
    label: str,
    predicate: Callable[[dict[str, Any]], bool],
    timeout: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout
    last: dict[str, Any] | None = None
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            last = get_snapshot(adb, package)
            if predicate(last):
                return last
        except (QualificationError, subprocess.TimeoutExpired) as error:
            last_error = error
        time.sleep(1.0)
    suffix = f"; last error: {last_error}" if last_error else f"; last snapshot: {last}"
    raise QualificationError(f"timed out waiting for {label}{suffix}")


def callback_count(snapshot: dict[str, Any]) -> int:
    return int(snapshot.get("audio_status", {}).get("callback_count", 0))


def wait_for_audio_progress(
    adb: Adb, package: str, label: str, timeout: float
) -> tuple[dict[str, Any], dict[str, Any]]:
    before = wait_for_snapshot(
        adb,
        package,
        f"{label} audio baseline",
        lambda value: bool(value.get("audio_running"))
        and bool(value.get("audio_status", {}).get("running")),
        timeout,
    )
    before_count = callback_count(before)
    after = wait_for_snapshot(
        adb,
        package,
        f"{label} audio callback progress",
        lambda value: bool(value.get("audio_running"))
        and callback_count(value) > before_count,
        timeout,
    )
    return before, after


def fingerprints(snapshot: dict[str, Any], field: str) -> set[str]:
    return {
        f"{entry.get('name', '')}|{entry.get('detail', '')}"
        for entry in snapshot.get(field, [])
    }


def usb_runtime_recovered(
    value: dict[str, Any],
    baseline_audio_label: str,
    baseline_open_ports: int,
    baseline_midi_generation: int,
) -> bool:
    status = value.get("audio_status", {})
    audio_matches = True
    if baseline_audio_label and baseline_audio_label != "System default":
        audio_matches = (
            value.get("selected_audio_output") == baseline_audio_label
            and int(value.get("selected_audio_device_id", -1)) > 0
            and int(status.get("device_id", -2))
            == int(value.get("selected_audio_device_id", -1))
        )
    midi_matches = baseline_open_ports == 0 or (
        int(value.get("open_midi_ports", 0)) >= baseline_open_ports
        and int(value.get("midi_generation", 0)) > baseline_midi_generation
    )
    return (
        bool(value.get("audio_running"))
        and bool(status.get("running"))
        and status.get("stream_health") == "healthy"
        and not bool(value.get("audio_recovery_in_progress"))
        and audio_matches
        and midi_matches
    )


def hardware_metadata(adb: Adb) -> dict[str, str]:
    properties = {
        "manufacturer": "ro.product.manufacturer",
        "model": "ro.product.model",
        "device": "ro.product.device",
        "android_release": "ro.build.version.release",
        "sdk": "ro.build.version.sdk",
        "build_fingerprint": "ro.build.fingerprint",
    }
    return {name: adb.shell("getprop", prop) for name, prop in properties.items()}


def record_stage(
    report: dict[str, Any], name: str, before: dict[str, Any], after: dict[str, Any]
) -> None:
    report["stages"].append({"name": name, "status": "passed", "before": before, "after": after})
    print(f"PASS  {name}")


def run_qualification(arguments: argparse.Namespace) -> dict[str, Any]:
    adb_path = locate_adb(arguments.adb)
    serial = choose_serial(adb_path, arguments.serial)
    adb = Adb(adb_path, serial)
    package = arguments.package
    component = f"{package}/.MainActivity"
    report: dict[str, Any] = {
        "schema_version": 1,
        "started_at": datetime.now(timezone.utc).isoformat(),
        "outcome": "running",
        "adb_serial": serial,
        "package": package,
        "hardware": hardware_metadata(adb),
        "usb_cycle_required": arguments.usb_cycle,
        "stages": [],
    }

    print("Launching RackForge and waiting for native audio…")
    adb.shell("am", "start", "-W", "-n", component, timeout=60.0)
    baseline_before, baseline_after = wait_for_audio_progress(
        adb, package, "startup", arguments.timeout
    )
    if not baseline_after.get("activity_resumed"):
        raise QualificationError("RackForge did not reach its resumed Activity state")
    record_stage(report, "startup", baseline_before, baseline_after)

    print("Locking the screen; audio callbacks must continue in the foreground service…")
    adb.shell("input", "keyevent", "KEYCODE_SLEEP")
    locked_before, locked_after = wait_for_audio_progress(
        adb, package, "screen lock", arguments.timeout
    )
    record_stage(report, "screen_lock", locked_before, locked_after)

    print("Waking and resuming RackForge…")
    adb.shell("input", "keyevent", "KEYCODE_WAKEUP")
    adb.shell("wm", "dismiss-keyguard")
    adb.shell("am", "start", "-W", "-n", component, timeout=60.0)
    resumed = wait_for_snapshot(
        adb,
        package,
        "Activity resume",
        lambda value: bool(value.get("activity_resumed"))
        and bool(value.get("audio_running")),
        arguments.timeout,
    )
    resume_before, resume_after = wait_for_audio_progress(
        adb, package, "screen resume", arguments.timeout
    )
    record_stage(report, "screen_resume", resumed or resume_before, resume_after)

    print("Sending RackForge to the background; native audio must remain live…")
    adb.shell("input", "keyevent", "KEYCODE_HOME")
    background = wait_for_snapshot(
        adb,
        package,
        "background Activity state",
        lambda value: not bool(value.get("activity_resumed")),
        arguments.timeout,
    )
    background_before, background_after = wait_for_audio_progress(
        adb, package, "background", arguments.timeout
    )
    record_stage(report, "background", background or background_before, background_after)

    adb.shell("am", "start", "-W", "-n", component, timeout=60.0)
    foreground = wait_for_snapshot(
        adb,
        package,
        "foreground resume",
        lambda value: bool(value.get("activity_resumed"))
        and bool(value.get("audio_running")),
        arguments.timeout,
    )
    foreground_before, foreground_after = wait_for_audio_progress(
        adb, package, "foreground resume", arguments.timeout
    )
    record_stage(report, "foreground_resume", foreground or foreground_before, foreground_after)

    if arguments.usb_cycle:
        usb_baseline = foreground_after
        baseline_usb = fingerprints(usb_baseline, "usb_devices")
        if not baseline_usb:
            raise QualificationError("--usb-cycle requires at least one connected USB device")
        baseline_midi_generation = int(usb_baseline.get("midi_generation", 0))
        baseline_open_ports = int(usb_baseline.get("open_midi_ports", 0))
        baseline_audio_label = str(usb_baseline.get("selected_audio_output", ""))

        print("DISCONNECT the USB hub/interface/controller now; waiting for removal…")
        disconnected = wait_for_snapshot(
            adb,
            package,
            "a baseline USB device to disappear",
            lambda value: bool(baseline_usb - fingerprints(value, "usb_devices")),
            arguments.usb_timeout,
        )
        removed_usb = baseline_usb - fingerprints(disconnected, "usb_devices")
        disconnected_before, disconnected_after = wait_for_audio_progress(
            adb, package, "USB fallback", arguments.timeout
        )
        record_stage(report, "usb_disconnect", disconnected_before, disconnected_after)

        print("RECONNECT the USB devices now; waiting for stable identities and audio…")
        reconnected = wait_for_snapshot(
            adb,
            package,
            "removed USB identities to reconnect",
            lambda value: removed_usb.issubset(fingerprints(value, "usb_devices")),
            arguments.usb_timeout,
        )

        recovered = wait_for_snapshot(
            adb,
            package,
            "USB audio and MIDI runtime recovery",
            lambda value: usb_runtime_recovered(
                value,
                baseline_audio_label,
                baseline_open_ports,
                baseline_midi_generation,
            ),
            arguments.usb_timeout,
        )
        recovered_before, recovered_after = wait_for_audio_progress(
            adb, package, "USB reconnect", arguments.timeout
        )
        record_stage(report, "usb_reconnect", reconnected or recovered_before, recovered_after)
        report["usb_removed_identities"] = sorted(removed_usb)
        report["usb_recovered_snapshot"] = recovered
    else:
        report["stages"].append(
            {
                "name": "usb_disconnect_reconnect",
                "status": "skipped",
                "reason": "run again with --usb-cycle for full hardware qualification",
            }
        )

    report["outcome"] = "passed" if arguments.usb_cycle else "passed_with_skips"
    report["finished_at"] = datetime.now(timezone.utc).isoformat()
    return report


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Exercise RackForge Android screen lock, background/resume, and optional "
            "physical USB disconnect/reconnect while verifying native audio progress."
        )
    )
    parser.add_argument("--adb", help="path to adb")
    parser.add_argument("--serial", help="ADB device serial; required when multiple are connected")
    parser.add_argument("--package", default=DEFAULT_PACKAGE)
    parser.add_argument("--timeout", type=float, default=30.0, help="seconds per lifecycle stage")
    parser.add_argument(
        "--usb-timeout", type=float, default=120.0, help="seconds allowed for each physical USB action"
    )
    parser.add_argument(
        "--usb-cycle",
        action="store_true",
        help="require physical USB disconnect/reconnect and validate MIDI/audio recovery",
    )
    parser.add_argument("--output", type=Path, help="JSON report path")
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = arguments.output or Path(
        f"dist/qualification/android-lifecycle-{timestamp}.json"
    )
    report: dict[str, Any]
    try:
        report = run_qualification(arguments)
    except (QualificationError, subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        report = {
            "schema_version": 1,
            "started_at": datetime.now(timezone.utc).isoformat(),
            "finished_at": datetime.now(timezone.utc).isoformat(),
            "outcome": "failed",
            "error": str(error),
        }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Report: {output.resolve()}")
    if report.get("outcome") not in {"passed", "passed_with_skips"}:
        print(f"FAIL  {report.get('error', 'qualification failed')}", file=sys.stderr)
        return 1
    if report["outcome"] == "passed":
        print("PASS  Full Android lifecycle and USB qualification")
    else:
        print("PASS  Android lifecycle smoke test (USB stage skipped)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
