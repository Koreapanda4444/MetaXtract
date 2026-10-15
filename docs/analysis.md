# Rust metadata analysis

[English](analysis.md) | [한국어](analysis_KR.md)

`metaxtract-analysis` consumes typed `ExtractionRecord` values independently of native parsers.
All analysis is read-only and has explicit field, result, byte and comparison budgets. Raw evidence
is retained; unsupported interpretations remain visible rather than being silently invented.

## Normalization

`normalize::normalize` returns additional claims, decoding status and issues. The caller may append
the claims before storing a new analysis record; it never overwrites source observations.
ISO/EXIF/PDF timestamps retain original spelling, precision and explicit offsets. Partial dates,
unknown timezones (`-00:00`) and leap seconds do not receive fabricated UTC instants. Windows
FILETIME uses checked integer arithmetic. GPS DMS, references, decimal positions, duration
nanoseconds and bitrate units are normalized. Stream and item scopes stay separate.
Identity display values are preserved; `identity_comparison_key` only collapses whitespace and
lowercases text, and does not assert two people are the same or perform Unicode normalization.
Encoding labels are canonicalized without re-decoding bytes; decoding belongs to C++.
Unknown fields remain in the original record. Unsupported date syntaxes produce explicit issues.

## Field lineage and timelines

`provenance::field_lineage` resolves supporting and dissenting observation IDs to original tags,
parser/library identities, virtual paths, streams, extraction methods and byte ranges. Invalid links
are rejected. `timeline` retains each conflicting claim and its resolution/confidence, plus separately
labelled filesystem events. Known instants sort chronologically; unresolved local/partial dates form
a separate trailing group and retain their original spelling and precision. This ordering does not
resolve their timezone or imply second-level accuracy for minute-precision dates.
