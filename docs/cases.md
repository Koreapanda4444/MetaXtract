# Case storage

`metaxtract-storage` owns the Rust single-file SQLite case workspace. It is separate
from the scanner's resumable task journal and does not load native parsers merely
to open a case. New cases are published atomically without replacing an existing
file. Transactions use a durable rollback journal and foreign keys.

Open a case with `CaseWorkspace::open(path, AccessMode::ReadOnly)` to inspect it
without migrations or recovery writes. A writable open upgrades known older Rust
schemas in one transaction; unknown future schemas are rejected. Python cases are
kept intact and imported into a new Rust case rather than changed in place.
