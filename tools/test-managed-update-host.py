#!/usr/bin/env python3
"""Prepare an isolated systemd host for update UI integration tests on Linux.

Requires a test-support updater, a complete native release tree, and sudo.
Only rackforge-updates-test-* roots, units and runtime directories are touched.
The RFC 8032 fixture key is public test material, never a production signing key.
"""
import argparse
import base64
import grp
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import pwd
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import time

TEST_PUBLIC = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo="
TEST_SECRET = bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
SUFFIXES = ["audio.path", "audio.service", "controller-host.service", "platform-host.service", "web.service", "updater.service"]


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, text=True, capture_output=True, **kwargs).stdout.strip()


def validate(root):
    root = root.resolve()
    if not root.name.startswith("rackforge-updates-test-") or root.parent in (Path("/"), Path("/etc"), Path("/usr")):
        raise ValueError("Integration roots must be dedicated rackforge-updates-test-* directories")
    if not all(char.isalnum() or char == "-" for char in root.name):
        raise ValueError("Invalid integration unit prefix")
    return root


def request(root, body):
    client = socket.socket(socket.AF_UNIX)
    client.settimeout(3)
    client.connect(f"/run/{root.name}/updater.sock")
    client.sendall(json.dumps(body).encode() + b"\n")
    with client, client.makefile("rb") as stream:
        return json.loads(stream.readline(65536))


def stop(root):
    names = [f"{root.name}-{suffix}" for suffix in SUFFIXES + ["installer.service"]]
    subprocess.run(["sudo", "-n", "systemctl", "stop", *names], capture_output=True)
    pid_file = root / "feed.pid"
    if pid_file.exists():
        pid = int(pid_file.read_text())
        command = Path(f"/proc/{pid}/cmdline")
        if command.exists() and str(root).encode() in command.read_bytes() and b"serve-feed" in command.read_bytes():
            os.kill(pid, signal.SIGTERM)


def prepare(args):
    root = validate(args.root)
    if root.exists():
        stop(root)
        root.rename(root.with_name(f"{root.name}-completed-{time.time_ns()}"))
    root.mkdir(parents=True)
    payload = args.payload.resolve()
    test_agent = args.test_agent.resolve()
    source = root / "fixture/rackforge"
    shutil.copytree(payload, source)
    (source / "config/update-public-key.txt").write_text(TEST_PUBLIC + "\n")
    shutil.copy2(test_agent, source / "target/release/rackforge-updater")
    fields = dict(line.split("=", 1) for line in (source / "build-info.txt").read_text().splitlines() if "=" in line)
    version = fields["version"]
    for name in ("bin", "config", "state/updates", "data", "controllers", "plugin-store", "logs"):
        (root / name).mkdir(parents=True, exist_ok=True)
    shutil.copytree(source / "target/release", root / "bin", dirs_exist_ok=True)
    shutil.copytree(source / "web/dist", root / "web")
    (root / "config/rackforge.toml").write_text(f'[web]\nenabled = true\naccess = "local"\nport = {args.web_port}\n')
    (root / "build-info.txt").write_text("version=0.1.39\nrevision=0123456\narchitecture=" + fields["architecture"] + "\nedition=standard\n")
    (root / "data/user-preset-sentinel.json").write_text('{"keep":"my settings"}\n')
    (root / "config/private-settings.json").write_text('{"keep":"my pin and device settings"}\n')
    if args.scenario == "bad-web":
        (source / "target/release/rackforge-web").write_text("#!/bin/sh\nexit 77\n")
        (source / "target/release/rackforge-web").chmod(0o755)
    assets = root / f"feed/releases/download/v{version}"
    assets.mkdir(parents=True)
    archive_name = "RackForge-RaspberryPi-arm64.tar.gz" if fields["architecture"] == "linux-aarch64" else "RackForge-Linux-x86_64.tar.gz"
    target = "linux-aarch64-services" if fields["architecture"] == "linux-aarch64" else "linux-x86_64-services"
    archive = assets / archive_name
    with tarfile.open(archive, "w:gz", compresslevel=1) as tar:
        tar.add(source, arcname="rackforge")
    metadata = dict(schema_version=1, product="org.rackforge.host", version=version,
                    notes=f"Isolated managed-update test: {args.scenario}",
                    targets={target: dict(url=f"https://github.com/kalexis1994/rackforge/releases/download/v{version}/{archive_name}",
                                          bytes=archive.stat().st_size, sha256=hashlib.sha256(archive.read_bytes()).hexdigest())})
    manifest = assets / "updates.json"
    manifest.write_text(json.dumps(metadata, indent=2) + "\n")
    secret = root / "fixture-test.key"
    secret.write_bytes(base64.b64encode(TEST_SECRET)); secret.chmod(0o600)
    run(test_agent, "sign", manifest, secret, assets / "updates.json.sig")
    run(test_agent, "verify", manifest, assets / "updates.json.sig", archive, target)
    secret.unlink()
    latest = root / "feed/releases/latest/download"
    latest.mkdir(parents=True)
    shutil.copy2(manifest, latest / "updates.json")
    if args.scenario == "bad-hash":
        with archive.open("r+b") as file:
            file.seek(20); file.write(b"tampered")
    account = pwd.getpwuid(os.getuid())
    group = grp.getgrgid(account.pw_gid).gr_name
    config = root / "agent.json"
    config.write_text(json.dumps(dict(root=str(root), user=account.pw_name, group=group,
                                     service_prefix=root.name, runtime=f"/run/{root.name}", web_port=args.web_port)))
    config_target = Path("/etc/rackforge-updater-tests") / f"{root.name}.json"
    run("sudo", "-n", "install", "-d", "-m", "0755", config_target.parent)
    run("sudo", "-n", "install", "-o", "root", "-g", "root", "-m", "0644", config, config_target)
    executable = Path("/usr/libexec") / f"{root.name}-agent"
    run("sudo", "-n", "install", "-o", "root", "-g", "root", "-m", "0755", test_agent, str(executable) + ".new")
    run("sudo", "-n", "mv", str(executable) + ".new", executable)
    prefix = root.name
    installer_name = f"{prefix}-installer.service"
    unit = f"""[Unit]
Description=Isolated RackForge update test agent
Before={' '.join(f'{prefix}-{suffix}' for suffix in SUFFIXES)}
[Service]
Type=notify
NotifyAccess=main
Group=rackforge
ExecStart={executable} supervise {config_target}
RuntimeDirectory={prefix}
RuntimeDirectoryMode=2770
Restart=always
RestartSec=1
TimeoutStartSec=120
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={root}
"""
    if args.scenario == "crash-web":
        unit += "Environment=RACKFORGE_UPDATE_FAILPOINT=web\n"
    units = {installer_name: unit}
    for suffix in SUFFIXES:
        name = f"{prefix}-{suffix}"
        common = f"[Unit]\nDescription=Isolated RackForge update test\nRequires={installer_name}\nAfter={installer_name}\n"
        if suffix == "audio.path":
            units[name] = common + f"[Path]\nPathExists={root}/never-create-audio-trigger\nUnit={prefix}-audio.service\n"
            continue
        env = f"Environment=RACKFORGE_ROOT={root}\nEnvironment=RACKFORGE_UPDATE_SOCKET=/run/{prefix}/updater.sock\nEnvironment=RACKFORGE_INSTALL_SOCKET=/run/{prefix}/control.sock\n"
        if suffix == "updater.service":
            env += f"Environment=RACKFORGE_UPDATE_TEST_FEED=http://127.0.0.1:{args.feed_port}\n"
            command = f"{root}/bin/rackforge-updater serve {root} /run/{prefix}/updater.sock"
        elif suffix == "web.service":
            command = f"{root}/bin/rackforge-web"
        else:
            command = "/usr/bin/sleep infinity"
        units[name] = common + f"[Service]\nUser={account.pw_name}\nGroup=rackforge\n{env}ExecStart={command}\nRestart=on-failure\nRestartSec=1\n"
    for name, text in units.items():
        stage = root / name
        stage.write_text(text)
        run("sudo", "-n", "install", "-m", "0644", stage, Path("/etc/systemd/system") / name)
    run("sudo", "-n", "systemctl", "daemon-reload")
    log = (root / "feed.log").open("ab")
    feed = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "serve-feed", "--root", str(root), "--feed-port", str(args.feed_port)], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    (root / "feed.pid").write_text(str(feed.pid))
    run("sudo", "-n", "systemctl", "start", installer_name, f"{prefix}-updater.service", f"{prefix}-web.service")
    for _ in range(100):
        try:
            state = request(root, {"op": "status"})
            if state["can_install"]:
                print(json.dumps(dict(root=str(root), scenario=args.scenario, state=state)), flush=True)
                return
        except (OSError, ValueError):
            pass
        time.sleep(.1)
    raise RuntimeError("Isolated update services did not start")


def check(root):
    state = request(root, {"op": "status"})
    assert (root / "data/user-preset-sentinel.json").read_text() == '{"keep":"my settings"}\n'
    assert (root / "config/private-settings.json").read_text() == '{"keep":"my pin and device settings"}\n'
    if state["phase"] in ("installed", "rolled_back"):
        journal = json.loads((root / "state/updates/activation.json").read_text())
        assert journal["status"]["phase"] == state["phase"]
        assert state["current_version"] == ("0.1.39" if state["phase"] == "rolled_back" else journal["status"]["target_version"])
    print(json.dumps(dict(state=state, user_data="preserved")), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["prepare", "check", "stop", "serve-feed"])
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--payload", type=Path)
    parser.add_argument("--test-agent", type=Path)
    parser.add_argument("--scenario", choices=["success", "bad-web", "bad-hash", "crash-web"], default="success")
    parser.add_argument("--web-port", type=int, default=18888)
    parser.add_argument("--feed-port", type=int, default=18889)
    args = parser.parse_args()
    root = validate(args.root)
    if args.operation == "prepare":
        prepare(args)
    elif args.operation == "check":
        check(root)
    elif args.operation == "stop":
        stop(root)
    else:
        class Handler(SimpleHTTPRequestHandler):
            def __init__(self, *a, **kw):
                super().__init__(*a, directory=str(root / "feed"), **kw)
        ThreadingHTTPServer(("127.0.0.1", args.feed_port), Handler).serve_forever()


if __name__ == "__main__":
    main()
