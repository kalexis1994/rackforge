#!/usr/bin/env python3
"""Builds the Concert Grand as one self-contained HTML page.

The page embeds the browser host, the engine worklet and the piano package —
base64, no server, no network after load — and boots the real engine in an
AudioWorklet with a playable keyboard on top. It exists for the situations
where RackForge cannot be served: trying a build from a machine with no
deployment, attaching a playable instrument to a bug report, or handing
someone the piano as a file.

Run tools/build-web-demo.sh first (it produces the host and the packages this
embeds), and build the site so the worklet chunk exists:

    tools/build-web-demo.sh
    VITE_RACKFORGE_BROWSER_HOST=1 pnpm --dir web build
    tools/build-live-page.py dist/concert-grand-live.html
"""

import base64
import glob
import json
import pathlib
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    root = pathlib.Path(__file__).resolve().parent.parent
    output = pathlib.Path(sys.argv[1])

    worklets = glob.glob(str(root / "web/dist/assets/engine.worklet-*.js"))
    if not worklets:
        print("no engine worklet in web/dist; build the site first", file=sys.stderr)
        return 2
    host = root / "web/public/demo/rackforge-browser.wasm"
    package = root / "web/public/demo/rackforge/plugins/concert-grand"
    if not host.is_file() or not package.is_dir():
        print("no demo assets; run tools/build-web-demo.sh first", file=sys.stderr)
        return 2

    files = {
        "plugins/concert-grand/" + path.relative_to(package).as_posix():
            base64.b64encode(path.read_bytes()).decode()
        for path in sorted(package.rglob("*"))
        if path.is_file()
    }
    page = (root / "tools/live-page/template.html").read_text()
    page = page.replace(
        "__WORKLET_B64__",
        base64.b64encode(pathlib.Path(worklets[0]).read_bytes()).decode(),
    )
    page = page.replace("__WASM_B64__", base64.b64encode(host.read_bytes()).decode())
    page = page.replace("__FILES_JSON__", json.dumps(files))

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(page)
    print(f"{output} ({output.stat().st_size / 1e6:.2f} MB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
