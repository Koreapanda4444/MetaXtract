"""Publish a version only after the complete CI workflow succeeds."""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tomllib
from pathlib import Path


def main() -> None:
    version = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]["package"]["version"]
    tag = "v" + version
    existing = subprocess.run(["gh", "release", "view", tag, "--json", "isDraft"], capture_output=True, text=True)
    if existing.returncode == 0:
        if not json.loads(existing.stdout)["isDraft"]:
            print("Version already published: " + tag)
            return
        raise ValueError("A draft already exists; inspect it before publishing")
    directory = Path("release-assets")
    subprocess.run(["gh", "run", "download", os.environ["VERIFIED_RUN"], "--pattern", "Rust-MetaXtract-*", "--dir", str(directory)], check=True)
    assets = sorted(p for p in directory.rglob("MetaXtract-*") if p.is_file() and not p.name.endswith(".sha256"))
    required = (".exe", ".zip", ".dmg", ".AppImage", ".deb")
    if not all(any(p.name.endswith(suffix) for p in assets) for suffix in required):
        raise ValueError("Verified CI did not provide all desktop formats")
    if len({p.name for p in assets}) != len(assets):
        raise ValueError("Release asset names are ambiguous")
    for archive in (p for p in assets if "-sources-" in p.name):
        if not archive.name.endswith(".tar.gz"):
            raise ValueError("Unexpected source asset")
    checksums = directory / "SHA256SUMS"
    checksums.write_text(''.join(hashlib.file_digest(p.open("rb"), "sha256").hexdigest() + "  " + p.name + "\n" for p in assets), encoding="ascii")
    notes = ("Desktop release: Windows Setup / portable ZIP, macOS DMG, Linux AppImage / DEB.\n\n"
             "데스크톱 배포: Windows 설치/포터블, macOS DMG, Linux AppImage/DEB.\n\n"
             "Verify SHA256SUMS before installing. Matching source archives and dependency notices are included. "
             "macOS CI builds use ad-hoc signatures and are not notarized; Windows builds are not Authenticode signed. "
             "See the included English/Korean installation guides.\n\nVerified CI: https://github.com/" + os.environ["GITHUB_REPOSITORY"] + "/actions/runs/" + os.environ["VERIFIED_RUN"])
    body = directory / "notes.md"
    body.write_text(notes, encoding="utf-8")
    subprocess.run(["gh", "release", "create", tag, "--target", os.environ["VERIFIED_SHA"], "--title", "MetaXtract " + version, "--notes-file", str(body), *map(str, assets), str(checksums)], check=True)


if __name__ == "__main__":
    main()
