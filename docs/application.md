# Rust application

[English](application.md) | [한국어](application_KR.md)

Build both the command-line application and bundled parser:

```bash
cargo build -p metaxtract-cli -p metaxtract-worker --locked
cargo run -p metaxtract-cli --locked -- --help
cargo run -p metaxtract-cli --locked -- scan /path/to/evidence --case investigation.mxcase --progress
cargo run -p metaxtract-cli --locked -- case history investigation.mxcase
cargo run -p metaxtract-cli --locked -- verify investigation.mxcase
```

Use `.exe` on Windows. Cargo launches configure native library search paths automatically. Standalone binaries require
`metaxtract-worker` plus the shared libraries and notices described in [dependencies](dependencies.md). `inspect FILE` creates an isolated temporary case. `compare CASE
OLD_SCAN NEW_SCAN` compares persisted scans. Case commands create/open information, history and
notes. Output is JSON; scan progress goes to stderr. Input paths retain native OS path units.

Scans persist records incrementally into a single-file case. A separate `.scan.sqlite` journal
retains active checkpoints; preserve it while a scan is paused. Resume using the original source,
`--resume RUN --scan CASE_SCAN_ID`, the same case and `--journal PATH` if customized. Completed
results are replayed with atomic receipts to close interruption gaps without duplicating records.
The journal must differ from the case and output files are excluded from discovery.

## Analysis and delivery

Use the scan ID returned by `scan` or `case history`:

```bash
cargo run -p metaxtract-cli -- search investigation.mxcase SCAN_ID --query 'mime:png' --limit 128
cargo run -p metaxtract-cli -- export investigation.mxcase SCAN_ID selected.csv --format csv
cargo run -p metaxtract-cli -- redact investigation.mxcase SCAN_ID private.jsonl
cargo run -p metaxtract-cli -- report investigation.mxcase SCAN_ID report.html --analyst Analyst
cargo run -p metaxtract-cli -- analyze investigation.mxcase SCAN_ID --view timeline
cargo run -p metaxtract-cli -- bundle export investigation.mxcase snapshot.zip
cargo run -p metaxtract-cli -- bundle open snapshot.zip reopened.mxcase
```

Search supports the structured grammar in [analysis](analysis.md); `--cursor` accepts the previous
page's `next` JSON and `--save NAME` persists the typed filter. `--order path|size|insertion` is
shared by search and exports. `redact --preview RECORD_ID` returns changes without writing a file.
A custom `--profile FILE` uses the existing redaction JSON schema; originals remain untouched.
`import CASE INPUT --kind jsonl|python|python-bundle` imports earlier scans and reference cases.

`keygen PRIVATE PUBLIC --name NAME --password-stdin` creates an encrypted Ed25519 private key.
Feed a password via stdin; a password argument is deliberately unavailable. For unattended usage,
`METAXTRACT_KEY_PASSWORD` is also accepted. Keep the public PEM independently trusted. `sign INPUT
SIGNATURE --key PRIVATE --password-stdin` signs exact bytes; `verify-signature INPUT SIGNATURE
PUBLIC` verifies them. Bundle export and reports accept the same key options. Signed reports
require `--receipt FILE` so their digest manifest and signature survive independently of console
output. Existing output files are never overwritten. Bundle verification distinguishes internal
integrity from independently trusted signatures; supply `--public-key PUBLIC` to authenticate.

## Command output contract

Successful commands write one JSON object to stdout: `{"ok":true,"command":"search","result":...}`.
`--pretty` changes whitespace only. Help is human-readable. Errors write one JSON object to stderr
with `ok:false`, `error.kind`, `error.message` and `error.exit_code`; stdout stays empty.
`--progress` emits newline-delimited `{"event":"scan","data":...}` events to stderr.
Unknown options, duplicate flags and excess positional arguments are rejected before case mutation.
Native paths are accepted for filesystem arguments; search text must be UTF-8.

Exit codes: **0** completed; **2** invalid arguments; **3** I/O or application failure; **4** invalid
data/integrity; **5** completed scan with failed/unstable files; **6** durably paused; **130** cancelled.
`verify-originals` also emits a completed result envelope and exits 4 when any original fails
verification; inspect `verified`, file counts and issues. A partial or paused scan still emits its successful result envelope, counts and checkpoint IDs.
A closed output pipe exits cleanly. `case history` and `search` return `items` plus `next`;
restart pagination after a case revision change. `--pause-after N` pauses cooperatively after at
least N completed files (already running work may finish); retain the journal and resume with the
returned run/scan IDs. Abrupt OS interruption leaves the journal available for recovery.

## Rust desktop

```bash
cargo build -p metaxtract-worker --locked
cargo run -p metaxtract-gui --locked
cargo run -p metaxtract-gui --locked -- investigation.mxcase
```

The native desktop starts with New/Open, recent cases, read-only opening, scan history, title and
notes. Writes persist in the local case; Save copy creates and verifies a consistent portable
snapshot before reopening the copy. The original case is retained. A bounded path picker accepts
typed paths and native directory entries without launching another program. Recent paths are
stored in the operating system's MetaXtract configuration directory.

Korean text uses Malgun Gothic on Windows, Apple SD Gothic Neo on macOS, or installed Noto Sans
CJK/Nanum Gothic on Linux. Set `METAXTRACT_FONT_PATH` to an installed TrueType/OpenType font if
needed. Linux requires a desktop display, OpenGL and libxkbcommon's X11 runtime
(`libxkbcommon-x11-0` on Ubuntu/Debian); `--smoke` opens the actual window and exits
after several rendered frames for installation checks. Case operations execute in background jobs
so the interface remains responsive. Original application artwork is preserved.

### Streaming scan queue

Open a writable case, choose **Scan queue**, browse to a file/directory and add it to the queue.
Up to 64 sources are queued with 1–32 workers per scan. Progress and counts update while each
completed file is persisted. **Pause** preserves a resumable checkpoint and holds the remaining
queue; **Cancel scan** marks the current scan cancelled. Queue removal affects waiting sources
only. Reopen the case and use **Resume checkpoint** for paused/interrupted scans; the receipt
selects the original source and journal with native paths and verifies their session binding.
Closing the window first pauses active work and waits for its checkpoint to finish. The waiting
queue is session-local; durable scan checkpoints remain in the separate journal.

### Paged results and selections

**Records** loads 64 lightweight headers per page, with path/size/discovery ordering and previous/next
controls. A changing case invalidates old cursors and restarts paging explicitly. The first page
refreshes during a scan. Opening one row loads only that full record. Checkboxes or Ctrl/Cmd-click
select up to 64 record IDs across pages; the count stays visible. Selected CSV/JSONL exports use
those exact IDs within the selected scan, including when several records share a path.
**Replace selected annotations** writes the entered note/tags for the entire selection in one
audited transaction. Existing annotations are replaced. Clear the selection before a different
batch. The structured CLI also accepts `id=NUMBER` for an exact record selection.

### Verified previews

Open a record and choose its original file (or an independently extracted nested member). The
selected regular file must match the record's size and SHA-256 before decoding. Preview reads
at most 64 MiB, retains at most 64 KiB of hex and 32 KiB of text, and uses a bounded temporary
snapshot and the bundled parser worker. It does not follow a path from an imported case.

C++ decodes JPEG/PNG/BMP/GIF/TIFF/WebP and available media codecs to their first RGB frame
(longest side 256 pixels); audio shows first-frame decode information. Unsupported codecs and
embedded display transforms are identified. OOXML/ODF previews show bounded body text. PDF
previews show the first three pages' Standard-14 Latin-font ASCII strings in content order.
Custom fonts/non-ASCII strings are explicitly omitted and page layout is not rendered. Full raw
metadata stays in the case; these display limits do not change it. Encrypted/unsupported input
still offers verified hex and diagnostics. `preview CASE RECORD_ID FILE` exposes the same
operation in the CLI. Existing checkpoints from another engine fingerprint are not resumed
under a changed parser build; start a new scan when the extraction engine changes.

Scan results retain the first 64 file-error summaries in `result.errors`; total failures remain in
`summary.failed`, and the complete per-file receipts stay in the case. Progress events also carry
`data.error`. A bundled-worker fingerprint mismatch explains that the application and worker
must be rebuilt together.

### Record inspection

A focused record has **Raw**, **Normalized**, **Provenance**, **Conflicts** and **Diagnostics** tabs
alongside Preview. Raw observations retain original values/encoded bytes, parser identity, packet,
stream and source ranges; interpretation is shown separately. Normalized claims show confidence,
resolution and supporting/contradicting observation IDs. Imported records are analyzed on a
read-only in-memory copy; failures leave the original available. Field lineage resolves exact
links, and conflict alternatives navigate to their claims. Large lists render only visible rows.
Detail JSON is capped at 16 KiB per view and binary previews at 128 bytes; complete data stays in
the case and exports.

### Search and comparison

**Search & compare** applies structured queries to the selected scan. Use whitespace AND, quoted
values and `-` negation, or the optional typed JSON editor for bounded AND/OR/NOT trees. Facet
summaries show MIME/status/claim/tag counts and explicitly indicate additional groups. Named
filters persist their full typed criteria; loading rebinds them to the selected scan. Legacy
criteria stay preserved and unknown status/query syntax requires manual review.

The comparison tab accepts before/after scan IDs and an optional shared restriction. It retains
the compared IDs, query and case revision, distinguishes added/removed/changed/moved records,
and opens each changed field's before/after value beside semantic claim changes. Binary changes
use length and SHA-256 rather than large byte arrays. Pause active scans for a consistent
comparison; a revision change aborts the operation.

### Analysis views

**Analysis** includes whole-case totals, a revision-bound dashboard, provenance-linked timeline,
offline longitude/latitude plot, exact hash groups, visual similarity and severity-filtered findings.
The current record filter or selected IDs define the analysis scope. File completion/failure counts
are persisted atomically with scan status and survive reopening; older imports may lack these counts.
GeoJSON/KML export contains all valid locations. The plot caps drawn points at 5,000 and exposes
all samples in the paged display. Invalid coordinates stay in diagnostics. Tracks require explicit
path-to-group assignments and resolved time zones; points are never linked by guesswork.

Enable **Decode previews** on a new scan to retain bounded native-decoded image fingerprints for
visual comparisons. Missing fingerprints are shown as coverage gaps. Similarity and anomaly flags
are leads for review and do not establish common authorship or tampering.

## Export and tools

The desktop **Export & tools** menu runs CSV/JSONL exports for the current filter or selected IDs,
custom redaction previews/profile saving, bilingual HTML/JSON reports and optional comparison
baselines. Redaction creates derivatives; the stored originals remain available. Signed reports
require a separate new receipt path. **Verify report receipt** checks the report's exact bytes and
signature against an independently trusted public PEM. The CLI equivalent is
`verify-report REPORT RECEIPT PUBLIC_PEM`.

Encrypted Ed25519 keys, exact-file signatures and trusted verification use masked password input;
the input is cleared when an operation starts and key passwords are never saved in preferences.
Public identities can be named and registered in a writable case. Complete portable case bundles
include original metadata/notes/annotations, irrespective of record filters or redaction settings;
they do not contain original evidence files. Bundle reopening creates a new destination. JSONL,
Python case and Python bundle import append to an existing writable case.

Audit verification accepts an optional independently retained anchor, exports the current anchor
and pages activity by sequence (128 entries per request). Parser diagnostics show current build
fingerprint, native registry/library versions, worker availability and stored scan engine details.
An engine change requires a new scan rather than resuming checkpoints under different parsers.

Background operations keep the UI responsive. Output paths must be new; existing files are not
overwritten. Pause an active scan before reports, bundle operations, imports or case switching.

## Investigation workflow

1. Extract the complete native package, launch the desktop and create a case outside the app
   folder. Keep original evidence in its own directory and record the package's build commit.
2. Add evidence to **Scan queue**. Enable **Decode previews** when visual similarity is needed.
   Retain the case and its `.scan.sqlite` journal while scans are active or paused.
3. Review record status, diagnostics, Raw and Provenance before using normalized claims. Review
   conflicting values and partial coverage in **Analysis**. Filters and selections restrict an
   operation; they do not remove other case records.
4. Pause the scan before reports or snapshots. Keep an unredacted record export for internal review
   and preview a separate redacted derivative when needed. Metadata redaction does not sanitize
   information embedded in an original evidence file.
5. Retain the case snapshot, original evidence and independently trusted public key/report receipt.
   Reopen the snapshot into a new case and verify it before removing a working copy. A portable
   case snapshot contains the database; original evidence is managed separately.

Replace `metaxtract` below with its full executable path, `./metaxtract` on Unix or
`.\metaxtract.exe` in Windows PowerShell. Commands return IDs for the following operation:

```text
metaxtract scan evidence --case investigation.mxcase --progress
metaxtract case history investigation.mxcase
metaxtract search investigation.mxcase SCAN_ID --limit 64
metaxtract verify investigation.mxcase
metaxtract report investigation.mxcase SCAN_ID report.html --analyst Analyst
metaxtract bundle export investigation.mxcase snapshot.zip
metaxtract bundle verify snapshot.zip
metaxtract bundle open snapshot.zip reopened.mxcase
```

**Import an earlier case:** create a writable Rust case, choose **Import into case**, and select
Python case/JSONL/Python bundle as appropriate. The CLI equivalent is `case create new.mxcase`,
then `import new.mxcase old.mxc --kind python`. Review the returned scan IDs and run `verify`
on the destination. Source cases stay unchanged; original imported rows and unsupported filter
syntax remain available for review. A signed Python bundle requires an independently trusted key.

**Recover an interrupted scan:** reopen the original case, retain its journal and evidence root,
then choose **Resume checkpoint**. CLI users supply the original `--resume RUN --scan SCAN_ID`
and custom `--journal PATH`, if any. An engine mismatch requires a new scan. If the journal was
lost, completed case records remain readable but the missing checkpoint cannot be reconstructed.

## Original verification, evidence delivery and cache

```bash
metaxtract scan evidence --case investigation.mxcase --include-hidden --decode-previews --cache on
metaxtract verify-originals investigation.mxcase SCAN_ID evidence
metaxtract export-evidence investigation.mxcase SCAN_ID evidence selected.zip --query 'path=evidence.txt'
metaxtract verify-evidence selected.zip
metaxtract analyze investigation.mxcase SCAN_ID --view locations --format geojson --output locations.geojson
metaxtract analyze investigation.mxcase SCAN_ID --view similar --threshold 8
metaxtract doctor --case investigation.mxcase
metaxtract cache purge investigation.mxcase.cache.sqlite
```

The desktop Workflows menu exposes original verification, selected evidence export, archive
integrity verification and cache purging. Scan queue options include hidden files, file limits
and cache use. Preview decoding during a scan is opt-in and retained decoded frames enable
visual similarity. Missing previews and conflicting orientation remain explicit coverage gaps.

A source root is authorized by the operator; case paths never authorize arbitrary disk access.
Original verification reopens regular root files within that root, refuses symlinks and compares
recorded size and SHA-256. Nested virtual records are counted separately and never opened as OS
paths. Selected evidence export requires physical root records; filter nested records out first.
The resulting ZIP contains typed metadata and unchanged original bytes, including their embedded
private metadata. It is distinct from a redacted metadata export and a portable case snapshot.
`verify-evidence` validates the complete canonical inventory and hashes without extracting files;
this proves internal integrity. Use `sign` and an independently trusted public key for authenticity.

Repeated exports of an unchanged case use fixed ZIP timestamps; snapshot `created_ms` is the case
content's last update time, not the wall-clock export time. The separate bounded cache stores raw
extraction results under content, engine and policy identities. `--cache off` bypasses it. Purging
accepts only a dedicated existing cache database and preserves case records and scan checkpoints.
