# Rust workspace

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

Rust owns the product-facing application: GUI and CLI, traversal, hashing, scheduling, caching,
normalization, analysis, search, reports, and case management.

## Current crates

| Crate | Implemented foundation |
|---|---|
| `metaxtract-schema` | Evidence identity, raw observations, normalized claims, provenance, diagnostics, and records |
| `metaxtract-native` | `cxx` bridge, bounded input, content detection, raw observation transport, parser capabilities, and native error mapping |
| `metaxtract-core` | Extraction orchestration, typed records, traversal, hashing, bounded scheduling, cache, durable scan storage and recovery |
| `metaxtract-worker` | Bundled internal parser process with a private read-only snapshot and framed checked output |
| `metaxtract-storage` | Single-file case workspaces, explicit read-only access, and transactional schema migrations |

The scanner is a Rust library API. Full case management, the Rust GUI and CLI remain pending.
The worker is an internal executable; it is launched by the core rather than used as a CLI.
Build from the repository root with `cargo build --workspace --locked`.

See [case storage](../docs/cases.md), [architecture](../docs/architecture.md), [the boundary contract](../docs/ffi.md), and
[conformance](../conformance/README.md).
