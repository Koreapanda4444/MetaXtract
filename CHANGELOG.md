# Changelog

## Current

### Added

- Added normalized metadata coverage for image orientation and frames, encrypted PDFs and
  document dates, extended DOCX properties, and detailed video and audio streams.
- Added JSON and HTML reports, scan diffs, privacy redaction, original-file verification,
  and complete case-bundle verification.
- Added deterministic case ZIP generation and per-artifact SHA-256 and size inventory.
- Added GUI background scanning with progress, cooperative cancellation, and visible errors.

### Changed

- Moved application code into the `src/metaxtract` package and grouped core, extractor,
  reporting, and case responsibilities into focused modules.
- Centralized package metadata, dependencies, pytest, and Ruff configuration in
  `pyproject.toml`; removed redundant wrappers and fixture-generation scripts.
- Consolidated tests around essential regression, security-boundary, and CLI workflow
  coverage while retaining only small checked-in format fixtures.
- Reworked the cache to reuse content hashes, isolate paths correctly, tolerate corrupt
  indexes, and batch writes once per scan.
- Standardized CLI dependency checks, error messages, and nonzero failure status.

### Security and reliability

- Reject symbolic links, absolute and traversal paths, scan-root escapes, duplicate paths,
  and case-insensitive archive collisions.
- Validate record shape and values before every report, diff, export, and verification
  operation.
- Bind declared originals and every non-manifest artifact to the bundle inventory.
- Bound JSONL size, line length, record count, ZIP size, member count, expanded size, and
  compression ratio before processing untrusted inputs.
- Create bundles atomically and reject originals that change after scanning.

### Validation

- Build and test a wheel in CI, then install that wheel into clean Linux, Windows, and macOS
  environments and run the full CLI workflow.
- Document the final source layout, supported commands, bundle guarantees, input limits,
  GUI behavior, and the purpose of the retained test suite.
