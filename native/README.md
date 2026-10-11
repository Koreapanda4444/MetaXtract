# Native extraction core

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

C++ is restricted to binary structure parsing, decoding, raw metadata extraction, and adapters
for approved native libraries. It does not own case state, normalization policy, analysis, search,
reporting, GUI, or CLI behavior.

The current `metaxtract-extract` core contains input handling, parser registration and dispatch,
resource budgets, and bridge support. Production format adapters and third-party extraction
library integrations are not implemented yet. The fixture parser exists only in conformance builds.
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
