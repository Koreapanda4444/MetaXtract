"""Native installers built from the same verified runtime tree."""
from __future__ import annotations

import os
import shutil
import struct
import subprocess
from pathlib import Path


def windows(root: Path, destination: Path, version: str) -> Path:
    compiler = shutil.which("makensis") or shutil.which("makensis", path=r"C:\Program Files (x86)\NSIS")
    if not compiler:
        raise ValueError("NSIS is required to build the Windows installer")
    icon = root.parent / "installer.ico"
    png = (Path(__file__).resolve().parents[1] / "crates/metaxtract-gui/assets/icon.png").read_bytes()
    icon.write_bytes(struct.pack("<HHHBBBBHHII", 0, 1, 1, 128, 128, 0, 0, 1, 32, len(png), 22) + png)
    target = destination / f"MetaXtract-{version}-Windows-Setup.exe"
    lines = [
        'Unicode true', '!include "MUI2.nsh"', 'Name "MetaXtract"',
        f'OutFile "{target.resolve()}"', 'RequestExecutionLevel user',
        'InstallDir "$LOCALAPPDATA\\Programs\\MetaXtract"',
        'InstallDirRegKey HKCU "Software\\MetaXtract" "InstallDir"',
        f'Icon "{icon}"', f'UninstallIcon "{icon}"',
        '!insertmacro MUI_PAGE_DIRECTORY', '!insertmacro MUI_PAGE_INSTFILES',
        '!insertmacro MUI_UNPAGE_CONFIRM', '!insertmacro MUI_UNPAGE_INSTFILES',
        '!insertmacro MUI_LANGUAGE "English"', '!insertmacro MUI_LANGUAGE "Korean"',
        'Section "MetaXtract"', 'SetShellVarContext current',
        'IfFileExists "$INSTDIR\\Uninstall.exe" 0 +2',
        'ExecWait \'"$INSTDIR\\Uninstall.exe" /S _?=$INSTDIR\'',
    ]
    files = sorted(p for p in root.rglob("*") if p.is_file())
    for path in files:
        relative = path.relative_to(root)
        parent = str(relative.parent).replace('/', '\\')
        lines += [f'SetOutPath "$INSTDIR\\{parent}"', f'File "{path.resolve()}"']
    lines += ['WriteUninstaller "$INSTDIR\\Uninstall.exe"',
              'WriteRegStr HKCU "Software\\MetaXtract" "InstallDir" "$INSTDIR"',
              'CreateDirectory "$SMPROGRAMS\\MetaXtract"',
              'CreateShortcut "$SMPROGRAMS\\MetaXtract\\MetaXtract.lnk" "$INSTDIR\\MetaXtract.exe" "" "$INSTDIR\\MetaXtract.exe" 0',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayName" "MetaXtract"',
              f'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayVersion" "{version}"',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayIcon" "$INSTDIR\\MetaXtract.exe,0"',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "UninstallString" \'$\\"$INSTDIR\\Uninstall.exe$\\"\'',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "QuietUninstallString" \'$\\"$INSTDIR\\Uninstall.exe$\\" /S\'',
              'SectionEnd',
              'Section /o "Desktop shortcut"',
              'CreateShortcut "$DESKTOP\\MetaXtract.lnk" "$INSTDIR\\MetaXtract.exe" "" "$INSTDIR\\MetaXtract.exe" 0', 'SectionEnd',
              'Section "Uninstall"', 'SetShellVarContext current']
    # Explicit ownership only: never recursively erase application/user directories.
    for path in files:
        relative = str(path.relative_to(root)).replace('/', '\\')
        lines += [f'Delete "$INSTDIR\\{relative}"']
    for directory in sorted((p for p in root.rglob("*") if p.is_dir()), key=lambda p: len(p.parts), reverse=True):
        relative = str(directory.relative_to(root)).replace('/', '\\')
        lines += [f'RMDir "$INSTDIR\\{relative}"']
    lines += ['Delete "$INSTDIR\\Uninstall.exe"', 'RMDir "$INSTDIR"',
              'DeleteRegKey HKCU "Software\\MetaXtract"',
              'DeleteRegKey HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract"',
              'Delete "$SMPROGRAMS\\MetaXtract\\MetaXtract.lnk"', 'RMDir "$SMPROGRAMS\\MetaXtract"',
              'Delete "$DESKTOP\\MetaXtract.lnk"', 'SectionEnd']
    script = root.parent / "installer.nsi"
    script.write_text('\n'.join(lines) + '\n', encoding="utf-8")
    subprocess.run([compiler, str(script)], check=True)
    return target
