# Standalone packages

[English](packaging.md) | [한국어](packaging_KR.md)

The Rust desktop, CLI and internal parser worker ship together with LibRaw, TagLib and the five
reviewed FFmpeg shared libraries. The other extraction libraries are linked into the executables.
Python, Rust, CMake, ExifTool, ffprobe and system-installed metadata libraries are not required
to run the package. Keep the complete extracted folder together.

## Run

| Platform | Desktop | CLI |
|---|---|---|
| Windows x64 | `MetaXtract.exe` | `runtime/bin/metaxtract.exe --help` |
| Linux x64 | `./MetaXtract` | `./runtime/bin/metaxtract --help` |
| macOS, package architecture | `MetaXtract.app` | `MetaXtract.app/Contents/MacOS/metaxtract --help` |

Extract the archive into a new user-owned folder before opening the desktop. Windows 10+ packages
include the MSVC app-local release runtime. The desktop also needs an OpenGL 2.0+ graphics driver.
Linux uses the operating system's glibc, C++ runtime
and desktop graphics services; `package.json` records the minimum required glibc and linked OS
libraries, including C++ ABI requirements. It does not promise compatibility with an older
distribution or a different CPU. The macOS bundle records its minimum OS version as well.
Linux needs a working X11/Wayland graphics environment and a Korean font for Korean text.
macOS uses system frameworks. Current CI packages are ad-hoc signed on macOS, without Developer
ID notarization; use the operating system's explicit Open action if Gatekeeper blocks the app.

The worker must stay beside the CLI/GUI executable. On Unix the executables locate the included
libraries through relative runtime paths; on Windows the DLLs stay beside the executables.
No environment variable or global library installation is needed for the analysis engines.

Save cases, scan journals, exports and evidence outside the application folder. Close the app and
all scans before replacing a package. Extract an upgrade into a separate folder, verify its
checksum and launch it against a copy of your case before replacing the old folder. Keep the old
folder until that check succeeds. Removing the application folder uninstalls this portable
package; it does not remove externally saved cases or per-user settings. Interrupted scans are
bound to the original engine fingerprint and may require a fresh scan after an engine change.

## Inventory and sources

The archive has a `.sha256` sidecar. `package.json` records the Git commit, target, raw file sizes
and SHA-256 checksums, binary paths, native source revisions and runtime dependencies. These
checks detect corruption; they are not a trusted publisher signature. `licenses/` contains native
notices, Rust dependency notices, the reviewed FFmpeg configuration and dependency locks.
Windows runtime files come unmodified from the build toolchain's licensed REDIST directory;
see Microsoft's [redistribution list](https://learn.microsoft.com/visualstudio/releases/2026/redistribution)
and [app-local deployment](https://learn.microsoft.com/cpp/windows/choosing-a-deployment-method).
App-local runtime updates are delivered with package updates.

A matching `MetaXtract-sources-<version>-<target>.tar.gz` contains application/build sources and the exact
upstream native source trees used in the build. The application's archive includes the libarchive
filter guard and allocator hooks, qpdf changes and FFmpeg build recipe. The original upstream
trees are unchanged. Shared libraries remain replaceable; there are no embedded metadata
executables. To rebuild using these native sources, set `CMAKE_TOOLCHAIN_FILE` to the absolute
path of `upstream-sources.cmake`, enter `application/`, and run `cargo build --workspace --release
--locked`. Cargo may need network access for its pinned Rust dependencies. The normal build
toolchain is still required to rebuild, but is not required to run the resulting package.

## Build a package

From a clean Git checkout with the documented Rust/C++ toolchain and Python 3.11+:

```bash
python packaging/build.py --check-source-archives
python packaging/build.py
```

The builder compiles the production workspace, finds the exact Cargo/native outputs, checks every
linked non-OS dependency, and writes the application and matching source archives under `dist/`.
Windows needs Visual Studio's x64 C++ tools and REDIST files; Unix needs `readelf` or `otool`.
`--allow-dirty` is for local verification and marks the inventory as uncommitted. It is not used
for CI release artifacts. Cross-compilation is not supported. Windows needs NSIS; Linux uses dpkg-deb and SHA-256-pinned AppImageTool 1.9.1 / type2 runtime 20251108. Optional macOS signing is described below.

## Installation verification

```bash
python packaging/verify.py --dist dist
python packaging/verify.py --dist dist --previous path/to/previous-artifact --gui-smoke
```

The Python verifier is a build/CI tool, not part of the installed application. It uses a private
folder with spaces and Korean characters, an empty tool PATH and no Cargo/Python/library-path
environment. It installs and verifies the archive and matching sources, extracts real PNG
metadata, decodes known pixels, and preserves a case's raw export, identity and notes across
replacement. It exercises failed-activation rollback, refuses unknown files/directories,
migrates a generated pre-audit schema-5 case, rejects a future schema without writing, and checks
that removal retains external evidence, cases and settings. GUI mode opens the persisted case
and preserves the desktop's actual recent-case settings as well.

Separate fresh CI jobs download the built package and a previous successful package for the
same OS/architecture when one is retained. They do not reuse the native build tree or install
a Python application.
`installation-verification.json` records both commits and distinguishes a previous-package upgrade
from a same-package reinstall. Reports are published in `Rust-installation-<OS>-<arch>` artifacts alongside the
`Rust-MetaXtract-<OS>-<arch>` package artifacts.
All three platforms exercise the packaged desktop; Linux uses Xvfb and software rendering.
Windows CI uses SHA-256-pinned [Mesa llvmpipe](https://github.com/pal1000/mesa-dist-win/releases/tag/26.2.4)
as its graphics driver. The verifier supplies that external driver directory only to GUI children;
the application inventory stays intact and CLI/worker checks keep their isolated library search.
The graphics mode is recorded as `desktop_graphics` in the receipt.

For main-branch changes limited to the installation verifier, this CI job, or project Markdown
guides, CI can download previously verified packages. It checks ancestry, the complete changed-file
list, unchanged build/sanitizer job settings, successful prior jobs and all three retained
native package artifacts. Other changes run the full builds. Fresh installation jobs always run
the current verifier. Reused packages retain their original build commit in their inventories
and matching source archives; the CI log identifies the source run and commit.
This checks an isolated runtime environment on hosted runners, not
every possible operating-system image, GPU driver or distribution. Python application wheel and
PyInstaller jobs have been removed; native acceptance retains the captured compatibility artifacts.

Download the `Rust-MetaXtract-<OS>-<arch>` artifact from its native build run. A verification-only
run identifies that source run in **Build or reuse verified packages** and publishes new
installation receipts. The application archive, its checksum and matching source archive belong
together. Historical `MetaXtract-Windows` artifacts belong to the retired Python application;
current CI publishes only the native product packages and verification receipts.

## Desktop installation, updates and removal

Use [GitHub Releases](https://github.com/Koreapanda4444/MetaXtract/releases) for stable versioned downloads. Releases are published after the complete CI workflow succeeds. Retain the matching source archive and notices. Compare the downloaded file against `SHA256SUMS` using `sha256sum`, `shasum -a 256`, or PowerShell `Get-FileHash -Algorithm SHA256`. Checksums detect corruption; they do not authenticate a publisher.

| Platform | Install and start | Upgrade / remove |
|---|---|---|
| Windows x64 | Run `MetaXtract-<version>-Windows-Setup.exe`; choose a per-user folder. Start-menu shortcut uses the embedded icon; desktop shortcut is optional. | Close the app and scans, then install at the same path. Settings → Apps removes owned files/shortcuts and retains user data. |
| Windows portable | Extract the release ZIP and open `MetaXtract.exe`. The CI `MetaXtract-Windows-Portable` artifact extracts directly into this layout. | Extract into a new folder and test a case copy before replacing the old folder. |
| macOS ARM64 | Open the DMG and drag `MetaXtract.app` to Applications. | Close and replace the bundle; remove it to uninstall. Retain downloaded sources/notices. |
| Linux x64 AppImage | `chmod +x MetaXtract-<version>-x86_64.AppImage`, then run. Without FUSE add `--appimage-extract-and-run`. | Close and replace the file; delete it to uninstall. |
| Linux amd64 DEB | `sudo apt install ./MetaXtract-<version>-amd64.deb`; open from the application menu. | Install the newer DEB; `sudo apt remove metaxtract` removes owned application/menu files. |

For a Linux extracted archive, `./desktop-integration.sh` registers the current path/icon in the user menu. Repeat after moving the folder; `./desktop-integration.sh --uninstall` removes the two integration files. Move paths containing desktop-file escape characters to a simpler directory first.

Windows/Linux internal GUI, CLI and worker live in `runtime/bin/`; Linux libraries are in `runtime/lib/`. The root launcher accepts `--cli` for the internal CLI. Keep the complete layout together. macOS embeds a copy of dependency notices inside the app bundle. AppImage still needs compatible OS graphics/font services and the glibc/C++ ABI recorded in `package.json`; DEB dependency floors derive from the built runtime.

Set `METAXTRACT_MACOS_IDENTITY` to a configured Developer ID identity before building to sign. Set `METAXTRACT_NOTARY_PROFILE` to an existing notarytool keychain profile for submission/stapling. Credentials stay in the build environment. Without these settings macOS is ad-hoc signed, **not notarized**. Windows EXEs/installers are **not Authenticode signed**. Review source, checksum and build provenance before using the OS's explicit trust controls.

Native-format verification checks NSIS install/reinstall/removal and embedded EXE resources, read-only DMG mounting and copied-app execution, AppImage extraction/execution and DEB install/reinstall/removal on disposable CI runners. GUI verification also exercises native scanning, filtered search, focused inspection, analysis, CSV/JSONL/HTML/JSON exports and source preservation. These checks complement the existing archive rollback and legacy-case compatibility verification.

Linux release binaries build on the explicit Ubuntu 24.04 runner to keep the glibc baseline stable; installation checks continue on ubuntu-latest to exercise forward compatibility. The measured ABI floor in package.json remains authoritative. Published releases include individual .sha256 files as well as SHA256SUMS.

Publication requires a `chore(release):` preparation commit on main. Its CI builds fresh native packages and matching sources; the publisher checks that the candidate is still the latest main commit before uploading assets and checksum files.

Use `--native-formats` only on a clean disposable test machine: native-format checks exercise actual shell/registry registration and the OS package manager. CI enables this option; ordinary archive/GUI verification remains in its private workspace. The DEB declares X11/Wayland runtime libraries, including libxkbcommon-x11, and recommends a desktop portal provider and Korean fonts.
