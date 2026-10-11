# Native extraction core

C++ is restricted to binary structure parsing, decoding, raw metadata extraction, and adapters
for approved native libraries. It does not own case state, normalization policy, analysis, search,
reporting, GUI, or CLI behavior.

The native build is introduced in migration commit 8. Its public boundary must remain smaller
than the parser implementations behind it.
