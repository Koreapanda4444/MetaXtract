# Python reference baseline

[English](README.md) | [한국어](README_KR.md) | [Project README](../../README.md)

The Python implementation at Git commit `374e79599fe8416652af4ee724780cf59537cc19`
is the behavioral reference for the Rust and C++ migration.

It is not the authority for metadata that the new engine did not previously expose. The
reference only protects existing user-visible behavior: safe relative paths, SHA-256 values,
record validation, case and bundle compatibility, CLI exit behavior, and the workflows already
available in the desktop application.

[`baseline.json`](baseline.json) records stable projections of representative outputs.
Volatile fields such as filesystem modification time are intentionally excluded.
The executable runner and migration corpus are in [`conformance/`](../../conformance/README.md).

The frozen source lives in [`legacy/python/`](../../legacy/python/README.md).
Its `src/` tree is preserved for comparison; documentation can be updated without changing the
reference implementation. See the [architecture](../../docs/architecture.md) for replacement gates.
