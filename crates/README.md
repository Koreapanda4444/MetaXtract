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
| `metaxtract-analysis` | Normalization, lineage, validation, findings, duplicates, geography, comparison, search, redaction, exports and reports |
| `metaxtract-storage` | Single-file cases, typed metadata, indexed paging, history, imports, audit, signing identities, and portable snapshots |
| `metaxtract-app` | Shared application workflows and bounded preview/scan operations |
| `metaxtract-cli` | Structured JSON commands, progress and stable exit codes |
| `metaxtract-gui` | Native desktop, paged inspection, analysis and delivery workflows |

Scanning, case storage and analysis are shared by the Rust GUI and CLI.
The worker is an internal executable; it is launched by the core rather than used as a CLI.
Build from the repository root with `cargo build --workspace --locked`.

See [case storage](../docs/cases.md), [architecture](../docs/architecture.md), [the boundary contract](../docs/ffi.md), and
[conformance](../conformance/README.md).

The GUI integrates scanning, paged inspection, search/comparison, analysis views and export/report/signing/bundle tools. See the [application guide](../docs/application.md).
