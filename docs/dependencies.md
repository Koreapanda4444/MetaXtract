# Dependency policy

Rust case transport uses exact-pinned `zip` and `ed25519-dalek` releases with
checksummed transitive packages in `Cargo.lock`. The transitive `fiat-crypto`
MIT/Apache-2.0/BSD-1-Clause license files were reviewed; BSD-1-Clause permits
redistribution and requires retaining its source copyright and disclaimer.

[English](dependencies.md) | [한국어](dependencies_KR.md) | [Project README](../README.md)

[`third-party.lock.toml`](../third-party.lock.toml) is the reviewed source-of-truth for native and bridge dependencies. Every
entry has an exact release, tag, and peeled commit revision. `python conformance/validate.py --online`
checks the tag against that commit, including annotated tags. A dependency is not enabled merely because it is
listed; the format-specific commit must still add a minimal adapter, verify its build options, and
record the notices shipped with the application.

Copyleft libraries permitted by [`dependency-policy.toml`](../dependency-policy.toml) are bundled dynamically so their license
and replacement requirements remain practical. FFmpeg must be configured without GPL or nonfree
components. Codec plugins pulled by libheif require their own review before enabling decoding.

Exiv2 is intentionally not selected while the project has no GPL-compatible distribution policy.
The image plan instead starts from focused libraries and MetaXtract-owned adapters. ExifTool and
ffprobe are forbidden as production runtime dependencies. The retired Python application is
absent from the final product tree; build/verification scripts do not ship as runtime services.

Dependency updates must change the version and revision together, rerun license validation, and
pass the malformed-file conformance corpus before merging.

[`Cargo.lock`](../Cargo.lock) pins active Rust dependencies and registry checksums.
`python conformance/validate.py --cargo` checks their declared SPDX licenses, including transitive build dependencies. This is a
declaration check; the notices and native build configuration still need review when an adapter
actually enables a library. Cargo builds the extraction core through the same CMake target used
by standalone native builds, then links the generated `cxx` bridge against that archive.

Run dependency validation from the repository root:

The active image metadata adapter statically bundles reviewed pugixml 1.16 and zlib 1.3.2.
CMake fetches their exact commits into `target/native-sources/<name>-<commit>` and reuses
those sources across Cargo profiles and standalone builds. Each use checks the Git commit
and rejects changes to tracked files; compiled objects remain in each build's own directory.
Git is required for these checks. Concurrent source fetches are locked, and CI caches only
these reviewed sources separately from media build artifacts. Explicit CMake source overrides
remain available. No system-library fallback or runtime download is used.
Installed builds include their original license notices
under `share/metaxtract/licenses`. Build tools and source fetches are build-time requirements.

The active RAW adapter dynamically bundles LibRaw 0.22.2, with LGPL/CDDL notices and the
original copyright list. Its pinned source is built without optional RawSpeed, DNG SDK,
JPEG or color-management dependencies. Cargo launches resolve it from the installed native
library directory; packaging a standalone Rust application must include this library and its
notices. Libheif remains reviewed but inactive; HEIF metadata currently uses bounded native
box/item parsing, without third-party pixel decoders.

```bash
python conformance/validate.py --online
python conformance/validate.py --cargo
```

The online check requires network access. The Cargo check requires the pinned Rust toolchain
and dependency metadata. See [conformance](../conformance/README.md) for the build environment.

The active audio adapter dynamically bundles TagLib 2.3.2 and its explicitly pinned UTFcpp
4.2.0 headers (Boost Software License 1.0). Implicit Git submodule fetching is disabled.
TagLib uses the reviewed zlib target; system dependencies and installed UTFcpp packages are
not substituted. Original LGPL/MPL and UTFcpp notices are included in installed builds.

The media adapter bundles shared FFmpeg 9.0.2 avformat, avcodec, avutil, swscale and
swresample libraries. Its build-only Python driver configures pinned sources and reviewed zlib.
It requires GNU make and a shell; Windows uses MSVC with MSYS2 bash/make. The new runtime
requires neither Python nor metadata executables. Installed builds include the original LGPL
notice and actual configuration manifest. The build cache includes source/zlib revisions,
compiler, CRT and sanitizer settings and uses a stable source path across Cargo profiles.

The PDF adapter statically bundles qpdf's locked library/native-crypto source lists and reviewed
libjpeg-turbo, with JPEG SIMD, tools and TurboJPEG disabled. It does not discover system zlib,
JPEG, OpenSSL or GnuTLS packages. Original Apache, IJG and libjpeg notices are installed.
Generated qpdf buffer/string pipelines and warning retention add parser-local budget hooks;
their pinned source and modifications remain identified. Original source files remain unchanged.
This software is based in part on the work of the Independent JPEG Group.

The archive adapter additionally bundles libarchive 3.8.9, liblzma from XZ 5.8.4 (0BSD), and libbzip2 1.0.8 (bzip2-1.0.6 license). These are source-pinned library-only builds; command-line tools, scripts, upstream tests and implicit host-library discovery are disabled. This software includes libbzip2 by Julian R. Seward and XZ Utils by its authors and contributors. Allocation interception is a MetaXtract build modification, while the reviewed upstream source cache remains unchanged.

The Rust desktop pins eframe/egui 0.32.3 with native Glow, X11 and Wayland support; web backends
and bundled file-dialog executables are disabled. The reviewed `libloading` ISC license permits
redistribution with its notice. Egui's original Ubuntu, Noto Emoji, Hack and emoji-icon fonts retain
the Ubuntu Font License 1.0, SIL OFL 1.1 and MIT/Bitstream notices in
`crates/metaxtract-gui/assets/third-party-notices.txt`. These allow embedding and redistribution
with their notices; original names and unmodified font data are preserved. Korean glyphs are
loaded from an installed system font (or `METAXTRACT_FONT_PATH`), which is not redistributed.
