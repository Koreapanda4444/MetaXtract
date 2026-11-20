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
        f'VIProductVersion "{version}.0"',
        'VIAddVersionKey /LANG=1033 "ProductName" "MetaXtract"',
        'VIAddVersionKey /LANG=1033 "FileDescription" "MetaXtract desktop installer"',
        f'VIAddVersionKey /LANG=1033 "FileVersion" "{version}"',
        f'VIAddVersionKey /LANG=1033 "ProductVersion" "{version}"',
        'VIAddVersionKey /LANG=1033 "LegalCopyright" "MetaXtract contributors"',
        'InstallDir "$LOCALAPPDATA\\Programs\\MetaXtract"',
        'InstallDirRegKey HKCU "Software\\MetaXtract" "InstallDir"',
        f'!define MUI_ICON "{icon}"', f'!define MUI_UNICON "{icon}"',
        '!insertmacro MUI_PAGE_COMPONENTS', '!insertmacro MUI_PAGE_DIRECTORY', '!insertmacro MUI_PAGE_INSTFILES',
        '!insertmacro MUI_UNPAGE_CONFIRM', '!insertmacro MUI_UNPAGE_INSTFILES',
        '!insertmacro MUI_LANGUAGE "English"', '!insertmacro MUI_LANGUAGE "Korean"',
        'LangString DesktopShortcut ${LANG_ENGLISH} "Desktop shortcut"',
        'LangString DesktopShortcut ${LANG_KOREAN} "바탕화면 바로가기"',
        'Section "MetaXtract"', 'SectionIn RO', 'SetShellVarContext current',
        'IfFileExists "$INSTDIR\\Uninstall.exe" 0 +2',
        'ExecWait \'"$INSTDIR\\Uninstall.exe" /S _?=$INSTDIR\'',
    ]
    files = sorted(p for p in root.rglob("*") if p.is_file())
    for path in files:
        relative = path.relative_to(root)
        parent = str(relative.parent).replace('/', '\\')
        lines += [f'SetOutPath "$INSTDIR\\{parent}"', f'File "{path.resolve()}"']
    lines += ['SetOutPath "$INSTDIR"', 'WriteUninstaller "$INSTDIR\\Uninstall.exe"',
              'WriteRegStr HKCU "Software\\MetaXtract" "InstallDir" "$INSTDIR"',
              'CreateDirectory "$SMPROGRAMS\\MetaXtract"',
              'CreateShortcut "$SMPROGRAMS\\MetaXtract\\MetaXtract.lnk" "$INSTDIR\\MetaXtract.exe" "" "$INSTDIR\\MetaXtract.exe" 0',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayName" "MetaXtract"',
              f'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayVersion" "{version}"',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "DisplayIcon" "$INSTDIR\\MetaXtract.exe,0"',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "UninstallString" \'$\\"$INSTDIR\\Uninstall.exe$\\"\'',
              'WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MetaXtract" "QuietUninstallString" \'$\\"$INSTDIR\\Uninstall.exe$\\" /S\'',
              'SectionEnd',
              'Section /o "$(DesktopShortcut)"',
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
    if png not in target.read_bytes():
        raise ValueError("NSIS omitted the original application icon")
    return target


def macos(root: Path, destination: Path, version: str) -> Path:
    app = root / "MetaXtract.app"
    identity = os.environ.get("METAXTRACT_MACOS_IDENTITY", "-")
    profile = os.environ.get("METAXTRACT_NOTARY_PROFILE")
    if profile and identity == "-":
        raise ValueError("Notarization requires a Developer ID signing identity")
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    stage = root.parent / "disk-image"
    shutil.copytree(root, stage)
    (stage / "Applications").symlink_to("/Applications", target_is_directory=True)
    image = destination / f"MetaXtract-{version}-macOS.dmg"
    subprocess.run(["hdiutil", "create", "-volname", "MetaXtract", "-srcfolder", str(stage), "-format", "UDZO", str(image)], check=True)
    if profile:
        subprocess.run(["xcrun", "notarytool", "submit", str(image), "--keychain-profile", profile, "--wait"], check=True)
        subprocess.run(["xcrun", "stapler", "staple", str(image)], check=True)
        subprocess.run(["xcrun", "stapler", "validate", str(image)], check=True)
    return image
