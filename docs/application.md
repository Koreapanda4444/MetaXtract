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

Use `.exe` on Windows. Keep `metaxtract-worker` and the reviewed native libraries next to the
application as built by Cargo. `inspect FILE` creates an isolated temporary case. `compare CASE
OLD_SCAN NEW_SCAN` compares persisted scans. Case commands create/open information, history and
notes. Output is JSON; scan progress goes to stderr. Input paths retain native OS path units.

Scans persist records incrementally into a single-file case. A separate `.scan.sqlite` journal
retains active checkpoints; preserve it while a scan is paused. Resume using the original source,
`--resume RUN --scan CASE_SCAN_ID`, the same case and `--journal PATH` if customized. Completed
results are replayed with atomic receipts to close interruption gaps without duplicating records.
The journal must differ from the case and output files are excluded from discovery.
