# Parser resource limits

Every native parser receives a fresh `ParserBudget`. Adapters must charge the budget before large
reads, allocations, decoded output, container descent, member enumeration, or observation output.
They must also call `checkpoint` inside long loops so the wall-time deadline is enforceable.

`run_parser` creates the context, rejects oversized sources before calling the adapter, catches
adapter exceptions, and checks the final budget even when an adapter returns success after a
failed budget operation. Reads go through `ParserContext`, which automatically charges bytes and
the two buffers used by the Rust callback and C++ result. Rust gives every parser an independent
read session; repeated or concurrent parsers do not consume one another's read budget.

The allocation ceiling is a conservative cumulative charged allocation budget. Third-party
internal allocations still require adapter-specific accounting or allocator hooks. Deadlines are
cooperative and are checked before and after an adapter call; they cannot forcibly interrupt a
native library call. Scanner process isolation will provide that separate hard boundary.

The default ceiling for one source is 1 GiB, with separate 64 MiB read, 256 MiB allocation, and
128 MiB decoded-output budgets. Container depth, entry count, observation count, and wall time are
limited independently. These are safety ceilings rather than format validity rules and can later
be exposed as scan-policy settings.

Crossing a limit returns a specific recoverable error. There are no production format adapters
yet; retaining partial observations will be part of their result transport. The conformance-only
fixture parser exercises every budget and the exception boundary through the same dispatcher.
