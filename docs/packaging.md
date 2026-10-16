# Standalone packages

[English](packaging.md) | [한국어](packaging_KR.md)

The Rust desktop, CLI and internal parser worker ship together with LibRaw, TagLib and the five
reviewed FFmpeg shared libraries. The other extraction libraries are linked into the executables.
Python, Rust, CMake, ExifTool, ffprobe and system-installed metadata libraries are not required
to run the package. Keep the complete extracted folder together.

## Run

| Platform | Desktop | CLI |
|---|---|---|
| Windows x64 | `metaxtract-gui.exe` | `metaxtract.exe --help` |
| Linux x64 | `./metaxtract-gui` | `./metaxtract --help` |
| macOS, package architecture | `MetaXtract.app` | `MetaXtract.app/Contents/MacOS/metaxtract --help` |

Extract the archive into a new user-owned folder before opening the desktop. Windows 10+ packages
include the MSVC app-local release runtime. Linux uses the operating system's glibc, C++ runtime
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

A matching `MetaXtract-sources-<target>.tar.gz` contains application/build sources and the exact
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
for CI release artifacts. Cross-compilation and public code signing are outside this builder.

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
the Python reference application.
`installation-verification.json` records both commits and distinguishes a previous-package upgrade
from a same-package reinstall. Reports are published in `Rust-installation-<OS>-<arch>` artifacts alongside the
`Rust-MetaXtract-<OS>-<arch>` package artifacts.
All three platforms exercise the packaged desktop; Linux uses Xvfb and software rendering.
This checks an isolated runtime environment on hosted runners, not
every possible operating-system image, GPU driver or distribution. Original Python reference
CI remains separate until the parity/removal stages.
