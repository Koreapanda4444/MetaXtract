# Native extraction core

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

C++ is restricted to binary structure parsing, decoding, raw metadata extraction, and adapters
for approved native libraries. It does not own case state, normalization policy, analysis, search,
reporting, GUI, or CLI behavior.

The extraction core implements input, budgets, registration and bridge support, image/RAW,
audio and media structure and deep metadata, and bounded in-process media decoding.
LibRaw, TagLib and FFmpeg use read-only Rust inputs. See the extraction guide for the exact scope.
The fixture parser exists only in conformance builds.
The public boundary must remain smaller than the parser implementations behind it.

From the repository root:

```bash
cmake --preset dev
cmake --build --preset dev --config Debug
ctest --preset dev -C Debug
```

Cargo builds the same core when compiling `metaxtract-native`. See
[the dependency policy](../docs/dependencies.md), [resource limits](../docs/resource-limits.md),
and [conformance](../conformance/README.md) for prerequisites and verification.
