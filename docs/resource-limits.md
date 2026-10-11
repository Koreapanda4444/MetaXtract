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
native library call. Scanner process isolation will provide that separate hard boundary.

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
