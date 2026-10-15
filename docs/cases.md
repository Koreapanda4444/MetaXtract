# Case storage

[English](cases.md) | [한국어](cases_KR.md) | [Project README](../README.md)

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

## Audit and integrity

Every case mutation records its actor, action, time, and changed-row commitments
in a SHA-256 chain in the same transaction. `with_actor` labels the writer;
`record_activity` adds explicit examiner activity. Updates process changed rows
instead of rehashing the whole case. `verify_integrity` replays the chain into a
temporary disk-backed index, compares it with current content, and checks payload
hashes, record references, foreign keys, and SQLite integrity.

Keep an `AuditAnchor` independently or authenticate it through a signed snapshot
to detect replacement of an entire case with an older copy. A self-contained
hash chain alone cannot prove authenticity against someone who can rewrite the
whole database. Migrated pre-audit data receives an explicit baseline anchor;
it is not represented as having a historical audit trail.

## Signing identities

`SigningIdentity` generates Ed25519 keys from the operating system RNG, exports
password-encrypted PKCS#8 PEM keys (PBES2, scrypt, AES-256-CBC), and signs exact
manifest bytes. Existing unencrypted Python PKCS#8 keys have an explicit import
method. Private key buffers are zeroized and key objects do not implement debug
printing. New key files are created privately and never replace an existing file.

Key decryption checks the encoded KDF parameters before doing expensive work.
`KeyDerivationLimits` defaults to 128 MiB of scrypt core memory, 8 Mi units of
scrypt work, and 5,000,000 PBKDF2 iterations. Named case identities contain only
public PEM keys and SHA-256 fingerprints; registering a different key under an
existing name is rejected and identity changes join the audit trail.

## Portable snapshots

`export_snapshot` takes a consistent SQLite backup, verifies its audit and payloads,
and streams the complete case into a ZIP containing `case.sqlite`, `manifest.json`,
and an optional Ed25519 `signature.json`. It includes scans, typed metadata,
history, notes, annotations, filters, import proofs, public identities, and audit
history. Private key files and source evidence outside the case are not embedded.
Export works on read-only cases and does not change their revision or audit head.

`verify_snapshot` checks the exact member inventory, sizes, hashes, signature trust,
case identity, schema, record count, and audit chain. `open_snapshot` publishes a
verified case at a new path without replacing an existing file. Supported older
Rust schemas migrate only in the temporary copy; the original bundle is unchanged.
The result reports the original schema and the resulting integrity anchor. Cases
that predate auditing receive a new explicit baseline, not a fabricated history.

All APIs are library APIs for the Rust application under development. The working
desktop interface remains the frozen Python reference until the GUI migration.
