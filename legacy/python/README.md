# MetaXtract Python reference

[English](README.md) | [한국어](README_KR.md) | [Project README](../../README.md)

MetaXtract collects metadata and SHA-256 hashes from local files, generates reports, and packages
results into verifiable case ZIPs. Analysis runs locally, with a CLI and a desktop GUI.

This guide describes the frozen Python application available during migration. See the
[project README](../../README.md) for the new Rust/C++ product's implementation status and design.
The optional `ffprobe` integration below belongs only to the Python reference and will not be
included in the new product.

## Features

- File and directory scanning with JSONL output
- Metadata normalization for images, PDF, DOCX, and video
- JSON and HTML reports, scan comparisons, and provenance-preserving timelines
- Offline GPS summaries and GeoJSON/KML exports
- Exact SHA-256 duplicates and visually similar images using perceptual hashes
- Privacy exposure and metadata anomaly classification with field-level evidence
- Path, size, and hash verification for originals and case bundles
- Metadata-only bundles with personal fields removed
- Deterministic ZIP output: identical inputs produce identical bytes
- A background GUI with single-file `.mxc` cases, autosave, recovery, history, notes, and annotations

## Requirements and installation

- Python 3.11 or newer
- Required Python packages: cryptography, Pillow, pypdf, python-docx
- Optional tool for the Python reference: `ffprobe` for video metadata extraction

```bash
git clone https://github.com/Koreapanda4444/MetaXtract.git
cd MetaXtract/legacy/python
```

Run the installation, development, and packaging commands in this guide from `legacy/python/`.
A virtual environment is optional if you want to isolate packages from other projects.
Skip creation and activation if you do not use one.

```bash
python -m venv .venv
```

Windows PowerShell:

```powershell
.venv\Scripts\Activate.ps1
python -m pip install --upgrade pip
python -m pip install .
```

Linux or macOS:

```bash
source .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install .
```

Check the installation with the following command. Features other than detailed video metadata
remain available without `ffprobe`.

```bash
metaxtract doctor
```

### Standalone Windows executable

Download the `MetaXtract-Windows` artifact from a successful GitHub Actions `CI` run to launch
`MetaXtract.exe` without installing Python or packages. The executable always uses the same
filename. It does not bundle `ffprobe`; install that separately if you need detailed video
metadata in the Python reference.

To build the same executable locally, run this in Windows PowerShell:

```powershell
python -m pip install -e ".[package]"
python -m PyInstaller --noconfirm --clean --onefile --windowed `
  --name MetaXtract `
  --icon src/metaxtract/assets/metaxtract.ico `
  --collect-submodules metaxtract `
  --collect-data metaxtract `
  src/metaxtract/gui.py
```

## Supported formats

| Format | Extensions | Main extracted fields |
|---|---|---|
| Images | `.jpg`, `.jpeg`, `.png` | Format, color mode, dimensions, frames, and EXIF including device, lens, orientation, and GPS |
| PDF | `.pdf` | Encryption status, page count, title, author, subject, keywords, and creation/modification information |
| DOCX | `.docx` | Paragraph/table/section counts and document properties including title, author, last editor, dates, and revision |
| Video | `.mp4`, `.mov`, `.m4v` | Container, duration, bitrate, streams, codecs, resolution, frame rate, and audio |

Other files still receive a relative path, size, modification time, MIME fallback, and SHA-256.

## Quick start

```bash
metaxtract scan evidence --out scan.jsonl
metaxtract report scan.jsonl --out report.json
metaxtract timeline scan.jsonl --out timeline.json
metaxtract geo scan.jsonl --format geojson --out locations.geojson
metaxtract duplicates scan.jsonl --files-base evidence --out duplicates.json
metaxtract findings scan.jsonl --out findings.json
metaxtract verify scan.jsonl evidence
metaxtract keygen private.pem public.pem
metaxtract export-case scan.jsonl case.zip --include-files --files-base evidence \
  --signing-key private.pem
metaxtract verify-bundle case.zip --public-key public.pem
```

The exit code is `0` on success and `1` for verification issues or command failures. Failures
print the command name and reason to standard error instead of exposing an exception traceback.

## Commands

### Scanning and cache

```bash
metaxtract scan evidence --out scan.jsonl
metaxtract scan evidence --cache off --max-files 1000
metaxtract scan evidence --include-hidden --out scan.jsonl
metaxtract cache purge
```

The default cache is `.metaxtract_cache/` in the current directory. Content hashes are calculated
once and the cache index is written once per scan. The default limit is 5,000 files.
`.git`, `__pycache__`, `.metaxtract_cache`, and hidden paths are excluded. Include hidden
paths with `--include-hidden`. Symlinks and paths escaping the scan root are rejected.

Each JSONL record has these fields:

```json
{"errors":[],"metadata":{},"mime":"application/octet-stream","path":"evidence.bin","sha256":"…","size_bytes":123,"warnings":[]}
```

### Reports and comparisons

```bash
metaxtract report scan.jsonl --out report.json
metaxtract report scan.jsonl --format html --out report.html
metaxtract diff old.jsonl new.jsonl --out diff.json
```

Reports include the file total, MIME/warning/error counts, and findings for GPS, authors,
creation software, and device models. `diff` distinguishes added, removed, and changed relative
paths. Changed files include before/after values for SHA-256, size, and nested metadata fields.

### Metadata timeline

```bash
metaxtract timeline scan.jsonl --out timeline.json
metaxtract timeline scan.jsonl --format jsonl --out timeline.jsonl
```

The timeline orders filesystem modification times, EXIF capture times, PDF creation/modification
times, DOCX creation/modification/print times, and video creation tags. Each event retains its
source path, meaning such as `created`, `modified`, or `captured`, original metadata field and
value, source kind, timezone presence, and precision. Values without timezones are not assigned
an arbitrary local timezone.

### GPS exports

```bash
metaxtract geo scan.jsonl --out locations.json
metaxtract geo scan.jsonl --format geojson --out locations.geojson
metaxtract geo scan.jsonl --format kml --out locations.kml
```

Only records with valid latitude and longitude are collected, together with their paths, hashes,
and original metadata fields. JSON includes the overall bounding box. GeoJSON and KML can be
opened in ordinary GIS or map applications. MetaXtract does not request map tiles or external
APIs, so this operation does not send GPS values over the network.

### Duplicate and similar files

```bash
metaxtract duplicates scan.jsonl --out duplicates.json
metaxtract duplicates scan.jsonl --files-base evidence --threshold 8 \
  --out duplicates.json
```

Exact duplicates group files with the same recorded SHA-256 and estimate reclaimable space.
With `--files-base`, the current size and SHA-256 of each original image are verified against
the scan before comparing 64-bit perceptual hashes. Pairs with a distance at or below
`--threshold` are reported as visually similar; exact duplicate pairs are not listed again.
Original files are not changed or sent to external services. Without an original directory,
only exact duplicates are calculated.

### Privacy exposure and anomalies

```bash
metaxtract findings scan.jsonl --out findings.json
```

GPS, author/user, device/serial number, and creation software fields are classified as privacy
exposure. Findings also cover extension/MIME mismatches, creation dates later than modification
dates, incomplete or out-of-range GPS, and file-processing errors. Each finding retains its
severity, rule ID, explanation, and the original fields and values used in the decision.
The same classifications appear in JSON and HTML reports.

### Original-file verification

```bash
metaxtract verify scan.jsonl evidence
```

Verification compares JSONL relative paths, file sizes, and SHA-256 values with the current
originals. Missing files, changes, duplicates, symlinks, and paths escaping the base directory
are reported separately.

### Case bundles

Metadata-only bundle:

```bash
metaxtract export-case scan.jsonl case.zip --case-id CASE-001 --notes "Initial scan"
```

Bundle including original files:

```bash
metaxtract export-case scan.jsonl case.zip \
  --include-files --files-base evidence
```

If `--files-base` is omitted, the directory containing `scan.jsonl` is used. Bundle creation
fails if an original's size or hash has changed since scanning, or if a path is unsafe.

Bundles use fixed member order, timestamps, permissions, and canonical JSON. The same records
and originals produce the same ZIP bytes. The `artifacts` section of `manifest.json` records
the size and SHA-256 of `scan.jsonl`, `hashes.txt`, reports, and each included original.
Only `manifest.json` itself is excluded from this inventory because it cannot hash itself.

Generate a key pair once to sign the manifest with Ed25519. Keep the private key private and
share only the public key with verifiers. Existing key files are not overwritten.

```bash
metaxtract keygen private.pem public.pem
metaxtract export-case scan.jsonl signed-case.zip --signing-key private.pem
metaxtract verify-bundle signed-case.zip --public-key public.pem
```

Signed bundles include `signature.json`. Identical inputs and the same key still produce
identical ZIP bytes.

Bundle with personal fields removed:

```bash
metaxtract export-case scan.jsonl redacted.zip --redact
```

`--redact` removes personal fields such as GPS, authors/editors, and EXIF capture times from
the bundled scan and report. Originals may still contain those fields, so `--redact` and
`--include-files` cannot be used together.

### Bundle verification

```bash
metaxtract verify-bundle case.zip
metaxtract verify-bundle case.zip --files-base evidence
metaxtract verify-bundle signed-case.zip --public-key public.pem
```

The verifier checks required members, duplicate/unsafe ZIP paths, JSONL records, the manifest
inventory, `hashes.txt`, reports, all artifact hashes, and included originals. Bundles without
originals can be compared with external files using `--files-base`. Signed bundles cannot
pass verification without a public key; the externally supplied key verifies the manifest
signature and key fingerprint.

Default limits for untrusted input:

| Target | Limit |
|---|---:|
| Total JSONL / line size / record count | 64 MiB / 1 MiB / 100,000 |
| ZIP file / member count | 8 GiB / 10,000 |
| ZIP member / total uncompressed size | 4 GiB / 16 GiB |
| ZIP control file / compression ratio | 64 MiB / 200:1 |

### GUI

```bash
metaxtract gui
```

The GUI requires Python's Tk support. Scans run in a background thread with progress and the
current file displayed. Cancellation is checked between chunks while hashing large files.
Scan/export failures appear in dialogs. JSONL is written completely to a temporary file before
replacement, so a failed save does not partially overwrite existing results.

Separate buttons select files and directories. Configure cache use, hidden files, and the maximum
file count in the interface. Results appear in a path/MIME/size/status table. Search and
OK/Warning/Error filters narrow the results; selecting a row shows its complete JSON.
Plain searches cover the record, tags, and notes. Structured queries include `mime:image`,
`size>=1048576`, `metadata.exif_make:canon`, `tag:review`, `note:"follow up"`, and
`status:error`. Conditions apply together. Save frequently used search/status combinations
under a name within the case.

Sort by column headers and select multiple rows with Ctrl/Shift. Batch actions copy selected
paths, add/remove tags, and clear annotations. Exports can cover all records, the current filtered
results, or selected rows. Export JSONL, JSON/HTML reports, or case ZIPs with optional originals,
redaction, and Ed25519 signing. CSV includes status, warnings/errors, metadata JSON, tags, and
notes; text that could be interpreted as a spreadsheet formula is handled safely.

`New` and `Open` manage single-file `.mxc` cases. When a case is open, completed records are
persisted incrementally during a scan so processed results can be reopened after interruption
or a crash. Switch previous scans with `Scan history` and save the case title/notes and
record tags/notes. `Compare` displays added/removed files and before/after field changes.
Import existing JSONL and verified case ZIPs as scan history; signed ZIPs require their public key.

`Dashboard` summarizes scans, sources, cumulative records, warnings, errors, and annotations.
It also shows the selected scan's total size, status/MIME distributions, changes from the previous
scan, case tags, and the full scan history.

`Analysis…` presents timeline, GPS, exact/visual duplicate, and privacy/anomaly tabs for the
current scan. Timeline and findings retain the original fields and values behind their results.
Export GeoJSON/KML from the location tab. Visual duplicate comparison runs only when explicitly
requested with local originals available, and runs as a separate job to keep the interface responsive.

Records with local originals support bounded previews of image thumbnails, the beginning of
PDFs, DOCX, and text. Previews first verify the current size and SHA-256 against the scan.
Escaping paths, symlinks, and excessive file/pixel/decompression sizes are rejected.

`Create Keys` generates Ed25519 private/public keys. `Verify ZIP` checks a case ZIP's structure,
hashes, and optional signature. Signed ZIPs require a public key; optionally compare an external
original directory as well.

## Project structure

Paths are relative to `legacy/python/`.

| Path | Purpose |
|---|---|
| `src/metaxtract/core/` | Scanning, cache, file/JSONL handling, record validation, and `.mxc` storage |
| `src/metaxtract/extractors/` | Format-specific extraction and metadata normalization |
| `src/metaxtract/case/` | Bundle creation/import, redaction, path checks, and integrity verification |
| `src/metaxtract/reporting/` | JSON/HTML reports, findings, and diff |
| `src/metaxtract/cli.py` | CLI routing and shared failure handling |
| `src/metaxtract/ui/` | Desktop views, dialogs, background tasks, and recent cases |
| `src/metaxtract/gui.py` | GUI compatibility entry point |
| `pyproject.toml` | Package, dependencies, build, and Ruff configuration |
| `../../.github/workflows/ci.yml` | Lint, builds, installed CLI checks, and Windows executable checks |

The Python reference has no separate `tests/`, `scripts/`, or duplicate configuration files.
Development tools and configuration live in `pyproject.toml`. The repository's
[conformance corpus](../../conformance/README.md) separately verifies the Rust/C++ migration.

## Development and builds

```bash
python -m pip install -e ".[dev]"
python -m ruff check .
python -m build --wheel
```

CI lints the source, builds a wheel, and installs only that wheel into fresh environments.
Installed CLI workflows run on Linux, Windows, and macOS with Python 3.11, and on Linux with
Python 3.14. A separate Windows job builds the standalone executable and checks that its GUI
process starts successfully.

## Limitations

- Extraction coverage depends on file condition, the libraries used, and format implementations.
- If `ffprobe` is absent or exceeds 30 seconds, detailed video metadata is skipped with a warning.
- Redaction applies only to structured metadata inside the bundle and does not modify originals.
- Unsigned ZIP and SHA-256 checks establish internal integrity. Ed25519 verifies that the manifest
  matches the supplied public key; it does not establish a trusted timestamp or the author's identity.
- The manifest hashes every other bundle artifact but not itself.
