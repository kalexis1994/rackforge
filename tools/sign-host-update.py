#!/usr/bin/env python3
"""Prepare signed host-update metadata for the managed Linux release packages.

The signing key stays outside the checkout. The signer verifies the resulting
metadata and both archives against the public key compiled into RackForge.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile

REPOSITORY = "https://github.com/kalexis1994/rackforge"
TARGETS = {
    "linux-x86_64-services": "RackForge-Linux-x86_64.tar.gz",
    "linux-aarch64-services": "RackForge-RaspberryPi-arm64.tar.gz",
}
PUBLIC_KEY = Path(__file__).resolve().parents[1] / "config/update-public-key.txt"


def manifest(assets: Path, version: str, notes: str) -> dict:
    version = version.removeprefix("v")
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version):
        raise ValueError("Managed updates require a stable semantic version")
    targets = {}
    for target, name in TARGETS.items():
        path = assets / name
        with tarfile.open(path, "r:gz") as archive:
            info = archive.extractfile("rackforge/build-info.txt")
            if info is None:
                raise ValueError(f"{name} has no build information")
            fields = dict(line.split("=", 1) for line in info.read(8192).decode("utf-8").splitlines() if "=" in line)
            architecture = "linux-aarch64" if target == "linux-aarch64-services" else "linux-x86-64"
            if fields.get("version") != version or fields.get("architecture") != architecture or fields.get("edition") != "standard":
                raise ValueError(f"{name} does not match version {version}, platform {target} and Standard edition")
            try:
                public_key = archive.extractfile("rackforge/config/update-public-key.txt")
            except KeyError as error:
                raise ValueError(f"{name} does not carry the host-update public key") from error
            if public_key is None or public_key.read(256).decode("utf-8").strip() != PUBLIC_KEY.read_text(encoding="utf-8").strip():
                raise ValueError(f"{name} was built for a different update signing key")
        with path.open("rb") as file:
            digest = hashlib.file_digest(file, "sha256").hexdigest()
        targets[target] = {"url": f"{REPOSITORY}/releases/download/v{version}/{name}",
                           "bytes": path.stat().st_size, "sha256": digest}
    return {"schema_version": 1, "product": "org.rackforge.host", "version": version,
            "notes": notes.encode("utf-8")[:16000].decode("utf-8", errors="ignore"), "targets": targets}


def sign(assets: Path, version: str, notes: str, secret_key: Path, signer: list[str]) -> None:
    if secret_key.resolve().is_relative_to(Path(__file__).resolve().parents[1]):
        raise ValueError("The private update key must be stored outside the checkout")
    metadata = assets / "updates.json"
    signature = assets / "updates.json.sig"
    if metadata.exists() or signature.exists():
        raise ValueError("Update metadata already exists; create a new release staging directory")
    metadata.write_text(json.dumps(manifest(assets, version, notes), indent=2) + "\n", encoding="utf-8", newline="\n")
    subprocess.run([*signer, "sign", str(metadata), str(secret_key), str(signature)], check=True)
    for target, name in TARGETS.items():
        subprocess.run([*signer, "verify", str(metadata), str(signature), str(assets / name), target], check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--notes", type=Path, required=True)
    parser.add_argument("--secret-key", type=Path, required=True)
    parser.add_argument("--signer", type=Path, help="Built rackforge-updater binary; defaults to cargo run")
    args = parser.parse_args()
    signer = [str(args.signer.resolve())] if args.signer else ["cargo", "run", "--locked", "--quiet", "-p", "rackforge-updater", "--"]
    sign(args.assets.resolve(), args.version, args.notes.read_text(encoding="utf-8"), args.secret_key.resolve(), signer)


if __name__ == "__main__":
    main()
