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

Rust input callbacks additionally cap an individual read at 1 MiB. These are safety ceilings
rather than format validity rules and can later be exposed as scan-policy settings.

Crossing a limit returns a specific recoverable error. The initial image adapter and extraction
transport retain observations emitted before a failure. The conformance-only
fixture parser exercises every budget and the exception boundary through the same dispatcher.

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

Each worker receives a private captured file whose hashes match the evidence read. It never
receives the original writable evidence handle. Timeout, cancellation, crash and malformed output
are caught by the parent; incomplete work is not cached as complete. This is crash/resource
isolation, not an OS security sandbox. Capture and verification add disk IO deliberately.

Completed records are batched up to sixteen records or about 16 MiB of estimated retained data.
A 25 ms idle flush prevents memory leases from waiting on a full batch. One transaction stores
result checksums and completion markers before delivering the records to the consumer.
