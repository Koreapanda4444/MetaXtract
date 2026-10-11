# Dependency policy

`third-party.lock.toml` is the reviewed source-of-truth for native and bridge dependencies. Every
entry has an exact release, tag, and peeled commit revision. `python conformance/validate.py --online`
checks the tag against that commit, including annotated tags. A dependency is not enabled merely because it is
listed; the format-specific commit must still add a minimal adapter, verify its build options, and
record the notices shipped with the application.

Copyleft libraries permitted by `dependency-policy.toml` are bundled dynamically so their license
and replacement requirements remain practical. FFmpeg must be configured without GPL or nonfree
components. Codec plugins pulled by libheif require their own review before enabling decoding.

Exiv2 is intentionally not selected while the project has no GPL-compatible distribution policy.
The image plan instead starts from focused libraries and MetaXtract-owned adapters. ExifTool and
ffprobe are forbidden as production runtime dependencies; their presence in the frozen Python
reference does not make them part of the new product.

Dependency updates must change the version and revision together, rerun license validation, and
pass the malformed-file conformance corpus before merging.

`Cargo.lock` pins active Rust dependencies and registry checksums. `python conformance/validate.py
--cargo` checks their declared SPDX licenses, including transitive build dependencies. This is a
declaration check; the notices and native build configuration still need review when an adapter
actually enables a library. Cargo builds the extraction core through the same CMake target used
by standalone native builds, then links the generated `cxx` bridge against that archive.
