# MetaXtract

[English](README.md) | [한국어](README_KR.md)

MetaXtract extracts metadata from local files and preserves the evidence behind its analysis.
The goal is broad format coverage, deep extraction, and accurate interpretation. Performance
and memory efficiency matter without sacrificing coverage or accuracy.

The application uses Rust and C++. Responsibilities are divided by task: C++ parses, decodes, and extracts raw metadata; Rust manages the application and analyzes
the results.

## Current status

The Rust desktop and CLI provide the current application workflows. The Rust/C++ engine
includes bounded input, schema/bridge contracts, content detection, deep image/RAW and audio
metadata, media inspection/decoding, PDF structure/security, Office documents and EML/MIME.

Native archive readers handle ZIP, TAR, 7z and common compressed streams. Rust recurses through
archives, attachments and OLE streams under shared limits and preserves virtual evidence paths.
Executable/script, font/ebook/package, DICOM and geospatial adapters retain raw metadata and
explicitly identify unsupported encodings or transforms.

Rust reconciles image claims and preserves unknown fields and separate backend claims.
The Rust core now provides safe traversal, streaming SHA-256/BLAKE3 hashing, bounded parallel
scanning, content-addressed caching, incremental SQLite persistence, durable pause/resume and
bundled parser-process isolation. IO buffers are reused, work is scheduled within a bounded cost
window, and completed records are committed in short batches.

The Rust [case storage library](docs/cases.md) provides a single-file workspace,
lossless metadata persistence, indexed paging, history and annotations, Python
case/JSONL/signed-bundle imports, hash-chained auditing, encrypted signing keys,
and verified portable case snapshots. It can open cases independently of native parsers.

The Rust desktop and CLI provide scanning, durable pause/resume, paged records, raw/provenance
inspection, structured search, comparison, timeline, offline locations, duplicates and findings.
The desktop also provides redaction previews, CSV/JSONL exports, HTML/JSON reports, encrypted
keys/signatures, case bundles, imports, audit verification and parser diagnostics.
See [native extraction coverage](docs/extraction.md) for supported fields and limitations.

The Python application has been retired after captured-field and workflow acceptance checks.
Its source remains in Git history; the input corpus, expected fields and genuine earlier case
artifacts remain in [conformance](conformance/README.md) for continued compatibility verification.

The Rust [analysis library](docs/analysis.md) provides normalization, field lineage and timelines,
claim validation, findings, exact/content/visual similarity, offline geographic analysis, scan/field
comparison, structured case search, redaction previews, selected CSV/JSONL/evidence exports, and
HTML/JSON investigation reports with optional signing receipts. GUI and CLI use the shared
application operations; see the application guide for available commands and analysis limits.

See the [Rust application guide](docs/application.md) for the new CLI and Rust desktop.

## Run the current application

Standalone CI packages include the desktop, CLI, parser worker, native libraries and notices.
Extract the complete package and launch `metaxtract-gui.exe` on Windows, `metaxtract-gui` on
Linux, or `MetaXtract.app` on macOS. See the [package guide](docs/packaging.md) for platform
requirements, installation, updates and source archives.

Build with the prerequisites below, then launch from the repository root:

```bash
cargo build --workspace --locked
cargo run -p metaxtract-gui --locked
# Optional: open a case directly
cargo run -p metaxtract-gui --locked -- investigation.mxcase
# CLI
cargo run -p metaxtract-cli --locked -- --help
```

Create/open a case, select **Scan queue**, choose a file or folder and start scanning. Use
**Records** to inspect results, **Analysis** for derived views and **Export & tools** for delivery.
Keep the adjacent `metaxtract-worker` and native libraries from the same build. Cargo configures
library paths when launching. Standalone distribution must include the libraries and license
notices in [dependencies](docs/dependencies.md). Linux needs an X11/Wayland desktop, OpenGL and
libxkbcommon, including its X11 library. On Ubuntu/Debian, install the GUI runtime with
`sudo apt-get install libxkbcommon-x11-0 libegl1 libgl1-mesa-dri fonts-noto-cjk`.
Install Noto Sans CJK or set `METAXTRACT_FONT_PATH` for Korean labels.

The application uses approved linked or bundled native libraries. It does not invoke external
metadata executables such as ExifTool or ffprobe, and it does not require Python at runtime.

## Build from source

Use the pinned Rust 1.88.0 toolchain, CMake 3.24 or newer, and a C++20 compiler. Windows builds
using Visual Studio 2026 require CMake 4.2 or newer; CI uses CMake 4.2.3.

Native builds also require Python 3.11+, GNU make and a shell. Windows needs MSVC and MSYS2
bash/make; set METAXTRACT_MSYS_ROOT if MSYS2 is outside C:/msys64.

Python is used only by build and verification scripts; no Python application is installed.
From the repository root:

```bash
cargo build --workspace --locked
python conformance/run.py
```

Conformance verifies defined contracts, eight captured reference inputs, earlier case artifacts
and application workflows. Passing it does not establish exhaustive equivalence for every format;
see the documented coverage and limits. No separate root test directory is required.

## Repository layout

| Path | Purpose |
|---|---|
| [`crates/`](crates/README.md) | Rust schema, native bridge, and application foundation |
| [`native/`](native/README.md) | C++ raw parsers, native library adapters, and bounded decoding |
| [`conformance/`](conformance/README.md) | Fixtures and compatibility checks for the migration |
| [`packaging/`](docs/packaging.md) | Build-time standalone assembly and dependency inspection |
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
| Case workspaces | [Cases](docs/cases.md) | [케이스](docs/cases_KR.md) |
| Analysis, search and reports | [Analysis](docs/analysis.md) | [분석·검색·보고서](docs/analysis_KR.md) |
| Migration verification | [Conformance](conformance/README.md) | [전환 검증](conformance/README_KR.md) |
| Current GUI and CLI | [Rust application](docs/application.md) | [Rust 앱 사용법](docs/application_KR.md) |
| Standalone installation and verification | [Packages](docs/packaging.md) | [독립 패키지](docs/packaging_KR.md) |
