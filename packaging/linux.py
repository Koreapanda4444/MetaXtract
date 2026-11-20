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
case "$root" in *'"'*|*'`'*|*'$'*|*'\\\\'*|*'%'*) echo 'Move the application to a simpler directory first.' >&2; exit 1;; esac
LC_ALL=C printf '%s' "$root" | LC_ALL=C grep -q '[[:cntrl:]]' && exit 1
sed "s|^Exec=.*|Exec=\\\"$root/MetaXtract\\\" %f|" "$root/share/applications/metaxtract.desktop" > "$entry"
cp -- "$root/share/icons/hicolor/128x128/apps/metaxtract.png" "$icon"
''', encoding="utf-8")
    (root / "desktop-integration.sh").chmod(0o755)
