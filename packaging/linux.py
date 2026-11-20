"""Linux desktop integration and distribution formats."""
from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import urllib.request
from pathlib import Path


def desktop(root: Path) -> None:
    applications = root / "share/applications"
    icons = root / "share/icons/hicolor/128x128/apps"
    applications.mkdir(parents=True)
    icons.mkdir(parents=True)
    shutil.copyfile(Path(__file__).resolve().parents[1] / "crates/metaxtract-gui/assets/icon.png", icons / "metaxtract.png")
    (applications / "metaxtract.desktop").write_text(
        '[Desktop Entry]\nType=Application\nName=MetaXtract\nComment=Inspect and analyze file metadata\n'
        'Comment[ko]=파일 메타데이터 검사 및 분석\nExec=MetaXtract %f\nIcon=metaxtract\n'
        'Terminal=false\nCategories=Utility;\nStartupWMClass=MetaXtract\n', encoding="utf-8")
    # User integration writes only two known files; the runtime remains portable.
    (root / "desktop-integration.sh").write_text('''#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
base=${XDG_DATA_HOME:-"$HOME/.local/share"}
entry="$base/applications/metaxtract.desktop"
icon="$base/icons/hicolor/128x128/apps/metaxtract.png"
if [ "${1:-}" = "--uninstall" ]; then
  rm -f -- "$entry" "$icon"
  exit 0
fi
mkdir -p -- "$(dirname -- "$entry")" "$(dirname -- "$icon")"
# Desktop Exec quoting: reject control characters and escape reserved characters.
case "$root" in *'"'*|*'`'*|*'$'*|*'\\\\'*|*'%'*|*'&'*|*'|'*) echo 'Move the application to a simpler directory first.' >&2; exit 1;; esac
LC_ALL=C printf '%s' "$root" | LC_ALL=C grep -q '[[:cntrl:]]' && exit 1
sed "s|^Exec=.*|Exec=\\\"$root/MetaXtract\\\" %f|" "$root/share/applications/metaxtract.desktop" > "$entry"
cp -- "$root/share/icons/hicolor/128x128/apps/metaxtract.png" "$icon"
''', encoding="utf-8")
    (root / "desktop-integration.sh").chmod(0o755)


def fetch(url: str, checksum: str, target: Path) -> Path:
    if not target.exists():
        with urllib.request.urlopen(url, timeout=60) as response, target.open("wb") as output:
            shutil.copyfileobj(response, output)
    if hashlib.sha256(target.read_bytes()).hexdigest() != checksum:
        raise ValueError("AppImage build input checksum differs: " + url)
    target.chmod(0o755)
    return target


def formats(root: Path, destination: Path, version: str, abi: dict) -> list[Path]:
    tools = root.parent / "appimage-tools"
    tools.mkdir()
    tool = fetch("https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage",
                 "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0", tools / "appimagetool")
    runtime = fetch("https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64",
                    "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d", tools / "runtime")
    appdir = root.parent / "MetaXtract.AppDir"
    shutil.copytree(root, appdir)
    shutil.copy2(appdir / "share/applications/metaxtract.desktop", appdir / "metaxtract.desktop")
    shutil.copy2(appdir / "share/icons/hicolor/128x128/apps/metaxtract.png", appdir / "metaxtract.png")
    (appdir / "AppRun").write_text('#!/bin/sh\nexec "${0%/*}/MetaXtract" "$@"\n', encoding="utf-8")
    (appdir / "AppRun").chmod(0o755)
    image = destination / f"MetaXtract-{version}-x86_64.AppImage"
    env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="1", ARCH="x86_64")
    subprocess.run([str(tool), "--runtime-file", str(runtime), str(appdir), str(image)], env=env, check=True)
    package = root.parent / "deb"
    application = package / "opt/MetaXtract"
    shutil.copytree(root, application)
    (package / "DEBIAN").mkdir()
    glibc = abi.get("glibc_minimum") or "2.39"
    glibcxx = abi.get("glibcxx_minimum") or "3.4.30"
    cpp = {30:"12",31:"13.1",32:"13.2",33:"14",34:"15",35:"16"}.get(int(glibcxx.split(".")[-1]))
    if cpp is None:
        raise ValueError("Review the libstdc++ package floor for " + glibcxx)
    dependencies = f"libc6 (>= {glibc}), libstdc++6 (>= {cpp}), libx11-6, libxkbcommon0, libegl1, libgl1, libwayland-client0, libwayland-cursor0, libwayland-egl1, libfontconfig1"
    (package / "DEBIAN/control").write_text(f"Package: metaxtract\nVersion: {version}\nArchitecture: amd64\nMaintainer: MetaXtract contributors\nSection: utils\nPriority: optional\nDepends: {dependencies}\nDescription: File metadata inspection and evidence analysis\n", encoding="utf-8")
    (package / "usr/bin").mkdir(parents=True)
    (package / "usr/bin/MetaXtract").symlink_to("/opt/MetaXtract/MetaXtract")
    shutil.copytree(root / "share", package / "usr/share")
    deb = destination / f"MetaXtract-{version}-amd64.deb"
    subprocess.run(["dpkg-deb", "--build", "--root-owner-group", str(package), str(deb)], check=True)
    return [image, deb]
