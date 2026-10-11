# MetaXtract

[English](README.md) | [한국어](README_KR.md)

MetaXtract extracts metadata from local files and preserves the evidence behind its analysis.
The goal is broad format coverage, deep extraction, and accurate interpretation. Performance
and memory efficiency matter without sacrificing coverage or accuracy.

The application is being migrated from Python to Rust and C++. Responsibilities are divided by
task: C++ parses, decodes, and extracts raw metadata; Rust manages the application and analyzes
the results.

## Current status

The Python reference provides the working GUI and CLI during migration. The Rust/C++ engine
now includes bounded input and schema/bridge contracts, content detection, deep image/RAW
and audio metadata, media container/track details, in-process inspection and bounded decoding.
PDF extraction adds Info/XMP, page geometry, attachments, revision chains, encryption settings,
signature ranges and active-content structures. Rust reconciles
image claims. The Rust GUI/CLI and full workflow parity remain pending;
see [native extraction coverage](docs/extraction.md) for supported fields and limitations.

The Python source remains frozen as a comparison baseline. It will be removed after the new
application passes workflow, data compatibility, and extraction conformance checks.

## Run the current application

From the repository root, with Python 3.11 or newer:

```bash
python -m pip install -e ./legacy/python
metaxtract gui
```

The GUI requires Python's Tk support. For CLI usage, run `metaxtract --help`. See the
[Python application guide](legacy/python/README.md) for installation, supported formats,
commands, and limitations.

The new Rust/C++ product must not depend on external metadata executables such as ExifTool or
ffprobe. Approved native libraries will be linked or bundled. The frozen Python reference still
has an optional ffprobe video backend; it is not part of the new architecture.

## Build the Rust/C++ foundation

Use the pinned Rust 1.88.0 toolchain, CMake 3.24 or newer, and a C++20 compiler. Windows builds
using Visual Studio 2026 require CMake 4.2 or newer; CI uses CMake 4.2.3.

Native builds also require Python 3.11+, GNU make and a shell. Windows needs MSVC and MSYS2
bash/make; set METAXTRACT_MSYS_ROOT if MSYS2 is outside C:/msys64.

Install the Python reference as above before running conformance checks. From the repository root:

```bash
cargo build --workspace --locked
python conformance/run.py
```

This builds and verifies the migration foundation. It does not launch a Rust desktop application
or establish metadata parity for format adapters that have not been implemented.

## Repository layout

| Path | Purpose |
|---|---|
| [`crates/`](crates/README.md) | Rust schema, native bridge, and application foundation |
| [`native/`](native/README.md) | C++ extraction core and future native library adapters |
| [`legacy/python/`](legacy/python/README.md) | Working Python application and frozen source reference |
| [`reference/python/`](reference/python/README.md) | Stable behavior captured from the Python application |
| [`conformance/`](conformance/README.md) | Fixtures and compatibility checks for the migration |
| `docs/` | Design decisions, dependency policy, and native safety contracts |

## Documentation

The README covers getting started. The guides in `docs/` explain decisions and constraints that
contributors need when changing the extraction engine. Project guides have English and Korean
counterparts; update both when behavior or design changes.

| Guide | English | 한국어 |
|---|---|---|
| Architecture and responsibilities | [Architecture](docs/architecture.md) | [아키텍처](docs/architecture_KR.md) |
| Language boundary and ownership | [Rust/C++ boundary](docs/ffi.md) | [Rust/C++ 경계](docs/ffi_KR.md) |
| Libraries and dependency policy | [Dependencies](docs/dependencies.md) | [의존성 정책](docs/dependencies_KR.md) |
| Parser budgets and limitations | [Resource limits](docs/resource-limits.md) | [파서 자원 제한](docs/resource-limits_KR.md) |
| Native extraction coverage | [Extraction](docs/extraction.md) | [추출 지원 범위](docs/extraction_KR.md) |
| Migration verification | [Conformance](conformance/README.md) | [전환 검증](conformance/README_KR.md) |
| Current GUI and CLI | [Python application](legacy/python/README.md) | [Python 앱 사용법](legacy/python/README_KR.md) |
