# Case storage

`metaxtract-storage` owns the Rust single-file SQLite case workspace. It is separate
from the scanner's resumable task journal and does not load native parsers merely
to open a case. New cases are published atomically without replacing an existing
file. Transactions use a durable rollback journal and foreign keys.

Open a case with `CaseWorkspace::open(path, AccessMode::ReadOnly)` to inspect it
without migrations or recovery writes. A writable open upgrades known older Rust
schemas in one transaction; unknown future schemas are rejected. Python cases are
kept intact and imported into a new Rust case rather than changed in place.

## Metadata records

Begin a scan with `begin_scan`, append `ExtractionRecord` values with
`append_record` or atomic `append_records`, and finalize it with `finish_scan`.
The typed record and separately indexed observations, claims, conflicts, and
diagnostics are committed together. Reads verify SHA-256 checksums. Raw bytes,
unknown tags, exact floating-point bits, provenance, and 128-bit times retain their
schema representation. Evidence paths are case-sensitive and preserve nested
container identity. A record has a 256 MiB serialization limit; exceeding it fails
the transaction without truncating metadata.

## Paged queries

`RecordQuery` filters a scan by status, MIME, a case-sensitive path prefix, a
canonical claim/value, conflicts, and an unsigned 64-bit size range. Pages contain
headers, not metadata blobs, and use keyset cursors for insertion, path, or size
ordering. Each page is limited to 256 rows and 16 MiB. A cursor is bound to the
case, its revision, and the query; restart after any case mutation. Retrieve a
complete record explicitly with `record(id)`.
