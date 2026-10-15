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
A partial or paused scan still emits its successful result envelope, counts and checkpoint IDs.
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
needed. Linux requires a desktop display and OpenGL; `--smoke` opens the actual window and exits
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
