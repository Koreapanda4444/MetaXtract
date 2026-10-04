# Changelog

## Unreleased

- Restored all CLI routes and added complete scan and bundle verification.
- Reworked cache storage to avoid collisions, mutation crashes, and duplicate entries.
- Added scan limits, hidden-file controls, and default repository/cache exclusions.
- Fixed EXIF GPS extraction and aligned report findings with extractor field names.
- Added real metadata redaction and rejected redacted bundles containing raw originals.
- Secured bundle paths against traversal, absolute paths, duplicates, and symbolic links.
- Added CLI workflow tests and replaced self-generating golden files with explicit checks.
- Migrated PDF extraction from deprecated PyPDF2 to pypdf.

## v0.1.0 - 2026-03-13

- Added fixed release version via metaxtract.__version__.
- Expanded README into a release-ready usage guide with install, format support, CLI examples, and limitations.
- Added developer automation commands for lint, test, and fixture regeneration.
- Polished project state for the first tagged release.
