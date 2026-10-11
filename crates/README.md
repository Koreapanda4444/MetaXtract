# Rust workspace

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

Rust owns the product-facing application: GUI and CLI, traversal, hashing, scheduling, caching,
normalization, analysis, search, reports, and case management.

## Current crates

| Crate | Implemented foundation |
|---|---|
| `metaxtract-schema` | Evidence identity, raw observations, normalized claims, provenance, diagnostics, and records |
| `metaxtract-native` | `cxx` bridge, bounded input, content detection, raw observation transport, parser capabilities, and native error mapping |
| `metaxtract-core` | Schema exports, engine information, and extension/MIME/container conflict analysis |

The workspace does not yet provide a Rust GUI, CLI, scanner, or case-management workflow.
These product responsibilities will be implemented above the schema and native bridge.
Build from the repository root with `cargo build --workspace --locked`.

See [architecture](../docs/architecture.md), [the boundary contract](../docs/ffi.md), and
[conformance](../conformance/README.md).
