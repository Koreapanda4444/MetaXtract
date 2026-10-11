# Dependency policy

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
ffprobe are forbidden as production runtime dependencies; their presence in the frozen Python
reference does not make them part of the new product.

Dependency updates must change the version and revision together, rerun license validation, and
pass the malformed-file conformance corpus before merging.

[`Cargo.lock`](../Cargo.lock) pins active Rust dependencies and registry checksums.
`python conformance/validate.py --cargo` checks their declared SPDX licenses, including transitive build dependencies. This is a
declaration check; the notices and native build configuration still need review when an adapter
actually enables a library. Cargo builds the extraction core through the same CMake target used
by standalone native builds, then links the generated `cxx` bridge against that archive.

Run dependency validation from the repository root:

The active image metadata adapter statically bundles reviewed pugixml 1.16 and zlib 1.3.2.
CMake fetches their exact commits from the lock into its build directory; no system-library
fallback or runtime download is used. Installed builds include their original license notices
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
