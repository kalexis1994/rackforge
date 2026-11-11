#!/usr/bin/env python3
"""Builds a `.rfplugin` used only to probe browser plugin lifecycle support.

Installation is a capability the browser host claims, so CI has to exercise it
against a real package. Rather than commit a binary for that, this packages a
component staged by the workflow and also emits a higher-version upgrade.

Usage: tools/make-probe-package.py <package-directory> <output.rfplugin>
"""

import json
import pathlib
import shutil
import sys
import tempfile
import zipfile

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

        runtime_path = staged / "metadata" / "runtime.json"
        runtime = json.loads(runtime_path.read_text())
        output.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(staged.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(staged).as_posix())
        # Keep both versions installed in the runtime probe: the catalog and
        # audio instance must agree on the newest package, with no duplicate ID.
        old_version = runtime["version"]
        new_version = f"{int(old_version.split('.')[0]) + 1}.0.0"
        manifest_path.write_text(manifest_path.read_text().replace(
            f'version = "{old_version}"', f'version = "{new_version}"', 1
        ))
        runtime["version"] = new_version
        runtime_path.write_text(json.dumps(runtime, indent=2) + "\n")
        upgrade = output.with_suffix(".upgrade.rfplugin")
        with zipfile.ZipFile(upgrade, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(staged.rglob("*")):
                if path.is_file():
                    archive.write(path, path.relative_to(staged).as_posix())
    print(f"{output} ({output.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
