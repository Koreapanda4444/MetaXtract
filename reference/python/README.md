# Python reference baseline

The Python implementation at Git commit `374e79599fe8416652af4ee724780cf59537cc19`
is the behavioral reference for the Rust and C++ migration.

It is not the authority for metadata that the new engine did not previously expose. The
reference only protects existing user-visible behavior: safe relative paths, SHA-256 values,
record validation, case and bundle compatibility, CLI exit behavior, and the workflows already
available in the desktop application.

`baseline.json` records stable projections of representative outputs. Volatile fields such as
filesystem modification time are intentionally excluded. Commit 12 adds the executable
conformance runner and the broader migration corpus.
