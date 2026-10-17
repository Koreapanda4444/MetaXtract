# MetaXtract architecture

[English](architecture.md) | [한국어](architecture_KR.md) | [Project README](../README.md)

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

GUI and CLI use the Rust core, which depends on the canonical schema and the Rust/C++ bridge.
The bridge calls the C++ extraction adapters.

The C++ core never calls into GUI, CLI, storage, reporting, or case-management code. Parser
libraries are hidden behind adapters and their objects never cross the language boundary.

## Extraction flow

The Rust desktop and CLI share these operations through `metaxtract-app`. The scanner uses the
bundled parser process and the native adapters listed in the extraction guide. The case database
and resumable scan journal have separate responsibilities and lifetimes.

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

## Retained compatibility evidence

The retired Python application remains in Git history. The final tree retains its source-tree
identity, captured input/output fields and genuine case/signed-bundle artifacts in conformance
data. Rust/C++ acceptance checks replay them through public application commands. The capture
is not authoritative for metadata the earlier application never extracted, nor an exhaustive
format oracle. Build-time Python scripts are separate from the Rust/C++ runtime.

[`architecture.toml`](../architecture.toml) is the machine-readable ownership policy used by
repository validation. See the [boundary contract](ffi.md), [dependency policy](dependencies.md),
and [resource limits](resource-limits.md) for the related implementation constraints.

## Evidence discovery

`metaxtract_core::scan::ScanRoot` accepts a regular file or directory. Child access is
relative to an opened directory capability, so following a link cannot grant access outside
the authorized root. The default policy skips symbolic links; `FollowWithinRoot` permits
links whose targets remain inside it. Directory identities prevent cycles and aliases.
Discovery limits cover files, entries, directories and depth. Dot-prefixed entries are
hidden by default; this is a name policy, not the Windows hidden-attribute policy.

Evidence records use portable relative paths while `NativePath` retains original Unix bytes
or Windows UTF-16 units for reopening. Unrepresentable names use reversible `~os-` encoding.
Opened-handle identity includes size, timestamps, device and file ID. Windows identity uses
the available 64-bit file index; it does not claim full 128-bit ReFS identity.

## Scan persistence and parser isolation

`scan_local` resolves the adjacent bundled worker. `scan_stream_with_control` accepts an explicit
backend for embedding; `NativeExtractor` is the controlled in-process implementation. Production
callers should use `BundledWorker`, optionally wrapped by `CachedExtractor`. The worker exchanges
only bounded, checksummed typed data and handles; it invokes no external metadata program.
Installed bundles must include the reviewed native libraries alongside the executable or in `lib/`.

`ScanStore::scan` commits tasks and records incrementally. `resume` uses a fresh `ScanControl`,
requeues unfinished tasks and rejects a different root or engine/backend fingerprint. Completed
records survive a crash. Pending evidence that changed is recorded as unstable rather than mixed
with earlier content. An invocation generation rejects stale writers when a scan is resumed.
Incomplete discovery is walked again; complete discovery only replays its durable pending tasks.
This scan store is separate from the single-file case workspace. `metaxtract-app` commits scan
receipts and records into that workspace, and both the desktop and CLI can reopen it. Existing
Python cases remain readable through the import operations in `metaxtract-storage`.

Cache keys include content hashes, filename hints, schema/API, compiled parser/library source
fingerprints and scan options. Checksums catch damaged payloads; partial results are not cached.
Typed storage keeps raw bytes, IEEE float bits and signed 128-bit timestamps without lossy JSON
conversion. Paths, ranges and confidence values are validated again when decoded.
