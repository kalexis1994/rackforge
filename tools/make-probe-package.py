#!/usr/bin/env python3
"""Builds a `.rfplugin` used only to probe installing and removing a plugin.

Installation is a capability the browser host claims, so CI has to exercise it
against a real package. Rather than commit a binary for that, this repackages
the demo instrument under a second identity: same component, different plugin
id, so installing it cannot collide with the copy the site already ships.

Usage: tools/make-probe-package.py <package-directory> <output.rfplugin>
"""

import json
import pathlib
import shutil
import sys
import tempfile
import zipfile

PROBE_ID = "org.rackforge.probe-instrument"
PROBE_NAME = "RackForge Probe Instrument"


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    source = pathlib.Path(sys.argv[1])
    output = pathlib.Path(sys.argv[2])

    with tempfile.TemporaryDirectory() as scratch:
        staged = pathlib.Path(scratch) / "package"
        shutil.copytree(source, staged)

        manifest_path = staged / "rackforge-plugin.toml"
        manifest = manifest_path.read_text()
        original_id = next(
            line.split("=", 1)[1].strip().strip('"')
            for line in manifest.splitlines()
            if line.startswith("id =")
        )
        manifest_path.write_text(
            manifest.replace(f'id = "{original_id}"', f'id = "{PROBE_ID}"').replace(
                manifest.split("name = ")[1].splitlines()[0],
                f'"{PROBE_NAME}"',
                1,
            )
        )

        runtime_path = staged / "metadata" / "runtime.json"
        runtime = json.loads(runtime_path.read_text())
        runtime["id"] = PROBE_ID
        runtime_path.write_text(json.dumps(runtime, indent=2) + "\n")

        output.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(staged.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(staged).as_posix())
    print(f"{output} ({output.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
