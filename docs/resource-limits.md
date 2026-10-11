# Parser resource limits

Every native parser receives a fresh `ParserBudget`. Adapters must charge the budget before large
reads, allocations, decoded output, container descent, member enumeration, or observation output.
They must also call `checkpoint` inside long loops so the wall-time deadline is enforceable.

The default ceiling for one source is 1 GiB, with separate 64 MiB read, 256 MiB allocation, and
128 MiB decoded-output budgets. Container depth, entry count, observation count, and wall time are
limited independently. These are safety ceilings rather than format validity rules and can later
be exposed as scan-policy settings.

Crossing a limit stops that parser and produces a specific recoverable diagnostic. Partial raw
observations already emitted remain evidence; another compatible parser may still run with its
own independent budget.
