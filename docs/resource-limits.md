# Parser resource limits

[English](resource-limits.md) | [한국어](resource-limits_KR.md) | [Project README](../README.md)

Every native parser receives a fresh `ParserBudget`. Adapters must charge the budget before large
reads, allocations, decoded output, container descent, member enumeration, or observation output.
They must also call `checkpoint` inside long loops to check the wall-time deadline cooperatively.

`run_parser` creates the context, rejects oversized sources before calling the adapter, catches
adapter exceptions, and checks the final budget even when an adapter returns success after a
failed budget operation. Reads go through `ParserContext`, which automatically charges bytes and
the two buffers used by the Rust callback and C++ result. Rust gives every parser an independent
read session; repeated or concurrent parsers do not consume one another's read budget.

The allocation ceiling is a conservative cumulative charged allocation budget, not a measurement
of peak resident memory. Third-party
internal allocations still require adapter-specific accounting or allocator hooks. Deadlines are
cooperative and are checked before and after an adapter call; they cannot forcibly interrupt a
native library call. The bundled scanner worker supplies the separate process boundary.

The current defaults in `ParserLimits` are:

| Resource | Default ceiling |
|---|---:|
| Source size | 1 GiB |
| Read bytes per parser | 64 MiB |
| Charged allocations per parser | 256 MiB |
| Decoded output per parser | 128 MiB |
| Container depth | 16 |
| Container entries | 10,000 |
| Observations | 250,000 |
| Wall time per parser | 30,000 ms |
| Nested member / selected stream size | 64 MiB |
| Decoded expansion relative to source | 1,000x |

Rust input callbacks additionally cap an individual read at 1 MiB. These are safety ceilings
rather than format validity rules. Rust callers can customize the policy; the CLI exposes the
four expansion limits listed below.

Crossing a limit returns a specific recoverable error. The initial image adapter and extraction
transport retain observations emitted before a failure. The conformance-only
fixture parser exercises every budget and the exception boundary through the same dispatcher.

Archive limits apply during decoding, before a child payload is allocated or queued. ZIP
declarations and sparse extents are checked before materialization; overlapping/backward blocks
are rejected. The pinned libarchive reader is built with guards on both normal and skipped
compressed filter reads, including compressed headers and special members. Filter stages and
materialized member output each charge decoded work, so layered compression cannot hide work
inside a library skip. A 64 KiB allowance, charged before entering the C library, keeps error
reporting possible when its ordinary allocation budget is exhausted.

Rust subtracts already charged read/allocation/decoded work across all parsers and descendants.
The expansion ceiling also uses the original root size for the whole tree; it does not reset at
each nested member. Byte arithmetic saturates or checks overflow before multiplication or copying.
Limit failures retain earlier observations and mark the scan as partial; they do not establish
that the remaining file is safe or empty. The native API revision is 2 for the additional policy
fields. Old serialized policies default these fields, and old case records remain readable.

`scan` and `inspect` accept `--max-member-bytes`, `--max-decoded-bytes`, `--max-tree-bytes`, and
`--max-expansion-ratio`. A ratio of `0` disables only the ratio ceiling; byte, time and worker
memory limits still apply. For example, `metaxtract inspect evidence.zip --max-expansion-ratio 2000`
permits more expansion while retaining the other defaults. The desktop currently uses the defaults.

See the [boundary contract](ffi.md) and [conformance guide](../conformance/README.md).

## Scanner policy

Default scan policy uses up to four worker threads, an eight-entry queue, a sixteen-file cost
window and a 1 MiB reusable hash buffer. Hashing and root parsing accept up to 1 GiB per source;
virtual inputs retain the separate 64 MiB/member and 256 MiB/scan-tree limits. The file deadline
is 120 seconds. Scan-wide deadlines are optional. Pause/cancel and ordinary file IO are cooperative.

A 4 GiB planned-memory budget admits jobs by conservative reservations. Result records have a
256 MiB retained-size ceiling; the wire/SQLite blob ceiling is 1 GiB. Reservations and retained-size
estimates are not measurements of aggregate RSS. Worker processes additionally enforce Linux
address-space or Windows Job Object process-memory limits. On macOS, large reserved allocator
zones prevent useful data-segment limits; the Rust parent samples physical footprint using the
native OS API, approximately every 10 ms, with possible brief overshoot. Sanitizer builds
set ASan's `hard_rss_limit_mb` because ASan reserves a multi-terabyte virtual shadow; its runtime
polls physical memory. Normal Linux builds retain the hard address-space limit.
See the [AddressSanitizer mapping note](https://clang.llvm.org/docs/AddressSanitizer.html#limitations).
For `METAXTRACT_SANITIZERS=address,undefined` builds, the worker explicitly declares ASan and
UBSan as executable link dependencies. The [Cargo link argument](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-arg)
from the native library's build script does not apply to dependent executables. ASan must be
linked before libc so the worker can start with its interceptors active.

Each worker receives a private captured file whose hashes match the evidence read. It never
receives the original writable evidence handle. The request uses a separate private control file;
stderr remains available for runtime diagnostics. The parent drains stderr while retaining at most
16 KiB for a failed task, so a crash includes its diagnostic without unbounded output buffering.
Timeout, cancellation, crash and malformed output
are caught by the parent; incomplete work is not cached as complete. This is crash/resource
isolation, not an OS security sandbox. Capture and verification add disk IO deliberately.

Completed records are batched up to sixteen records or about 16 MiB of estimated retained data.
A 25 ms idle flush prevents memory leases from waiting on a full batch. One transaction stores
result checksums and completion markers before delivering the records to the consumer.
