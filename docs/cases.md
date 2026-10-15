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

## History and case work

`scan_history` pages scan origins, statuses, times, and diagnostic counts. Paused
case scans resume explicitly; opening a case never marks another live scanner as
failed. Case notes, individual authored notes, record annotations with bounded
tags, and named filters are transactional. Structured filters retain `RecordQuery`;
imported Python filters retain their original query text and status separately.

## Imports

`import_jsonl` accepts Python scan rows and typed Rust extraction records.
`import_python_case` reads schema 1–3 Python cases through a consistent SQLite
backup without migrating or editing the source. Runs, notes, annotations, and
saved filters are retained. Original imported rows and case details remain
available in import receipts; the importer labels legacy observations rather
than inventing native provenance or normalized claims.

`import_python_bundle` checks all member paths, CRCs, expansion limits, inventory
sizes and SHA-256 hashes, scan counts, and the hash list. A signed bundle requires
an explicitly trusted Ed25519 public PEM key; supplying a key also rejects an
unsigned bundle. Embedded original evidence is verified, while this metadata
import keeps the original files in the source bundle. The exact manifest,
signature, and trusted-key receipt remain in the case.

`ImportLimits` bounds input size, member size/count, expanded bytes, record size/
count, and controls. Imports are atomic and fail without partial records.
`append_records_with_receipt` can also attach a scanner's serialized native path,
file identity, hashes, and failure context atomically to its extraction records.
