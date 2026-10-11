# MetaXtract architecture

## Product boundary

MetaXtract is a metadata extraction and analysis application. Binary parsing exists only to
locate, decode, validate, and explain metadata. A general-purpose hex editor, disassembler,
debugger, or arbitrary binary-analysis workbench is outside the product boundary.

## Ownership

Rust owns all durable state and user-visible policy:

- GUI and CLI behavior
- file discovery, identity, hashing, and integrity checks
- scheduling, cancellation, cache keys, checkpoints, and resource policy
- canonical schema, normalization, correlation, and anomaly detection
- case storage, search, comparison, redaction, reports, and portable bundles

C++ owns the unsafe native extraction kernel:

- binary structure parsing required to reach metadata
- data decoding required to interpret metadata values or previews
- lossless raw metadata extraction
- adapters around approved native libraries

C++ does not decide which conflicting value is true, write case files, walk user directories,
perform network access, or implement product workflows.

## Dependency direction

```text
GUI / CLI -> Rust core -> canonical schema
                      -> Rust/C++ bridge -> C++ extraction adapters
```

The C++ core never calls into GUI, CLI, storage, reporting, or case-management code. Parser
libraries are hidden behind adapters and their objects never cross the language boundary.

## Extraction flow

1. Rust resolves the scan target and opens a bounded read-only source.
2. Rust records file identity and hashes content according to evidence policy.
3. C++ probes compatible parsers and emits independent raw observations.
4. Rust validates limits and converts observations into canonical evidence.
5. Rust normalizes claims, retains disagreement, and performs analysis.
6. Rust persists and presents results without changing the source file.

The bridge transports values and diagnostics, not serialized C++ object graphs. Every result
identifies its parser, source region when known, and decoding status. Exceptions are caught on
the C++ side and converted to structured failures.

## Runtime dependencies

MetaXtract may link or bundle approved libraries. It must not invoke ExifTool, ffprobe, or another
user-installed metadata executable at runtime. Build tools are not runtime dependencies.

## Python reference

The frozen Python application is an executable compatibility oracle for existing workflows. It is
not authoritative for fields it never extracted. Rust and C++ replace it only after conformance,
workspace compatibility, and end-to-end acceptance gates pass.

`architecture.toml` is the machine-readable ownership policy used by repository validation.
