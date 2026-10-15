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

## Claim validation

`validation::reconcile` derives support, dissent, conflicts, confidence and resolution from linked
observations, never from unverified parser-name lists. Wrappers sharing a library count as one
family; builtin parser prefixes conservatively identify a shared implementation. Unknown zones
and overlapping timestamp precision are indeterminate, not contradictions. Different known
instants or values remain explicit alternatives. Confidence is an evidence classification score,
not a statistical probability. User selections and rejected alternatives remain identified.

## Findings

`findings::classify` returns stable rule IDs, severity, category, source path and supporting claim/
observation IDs. Rules cover location and identity exposure, local paths, conflicting claims,
lossy decoding, incomplete extraction, type mismatch, active content and unusual chronology.
Tampering indicators are explicitly heuristic; they are not conclusions of forgery or malware.
No external network enrichment is performed, and original observations remain reviewable.

## Duplicate analysis

`duplicates::exact` groups full SHA-256/BLAKE3 digests with consistent sizes, separately by algorithm.
`fingerprint_content` streams caller-opened evidence, verifies the recorded size and SHA-256, and
constructs content-defined chunk hashes. `near` compares chunk-set Jaccard similarity; it is a
candidate heuristic, especially limited for short files, and does not establish byte equality.
`visual_fingerprints` consumes C++ RGB24 preview observations, checks dimensions/length, and
computes an explicitly oriented 64-bit dHash. `visually_similar` applies a Hamming threshold.
No image decoder or external executable is added to Rust. Missing native previews yield no
visual fingerprint; callers must report that coverage gap. Different algorithm scores are not
interchangeable, and similarity never authorizes automatic evidence deletion.

## Offline geographic analysis

`geo::locations` preserves claim/observation references and associates only unambiguous capture
times in the same scope. `clusters` computes radius-connected components with a spherical
centroid that handles the antimeridian; it is single-link clustering, not DBSCAN. Distances use a
spherical Earth approximation. `tracks` accepts explicit path-to-group assignments, splits long
time gaps, and flags excessive speed or simultaneous different positions. Ungrouped paths stay
separate; missing/ambiguous UTC times remain unresolved. Track indices reference the input sample
list. GeoJSON uses longitude/latitude order; KML escapes source text. No online tiles or geocoding
are requested. GPS altitude references below sea level are preserved.

## Scan and field comparison

`diff::compare_records` distinguishes missing values from explicit null, uses typed key/index path
segments, and reports original-observation changes separately from canonical claim alternatives.
Claim comparisons identify equivalent values even when provenance or IDs changed; unknown time
zones remain indeterminate. Large binary fields are represented by byte length and SHA-256 in
the diff, with originals retained in the case. `compare_scans` rejects duplicate evidence paths,
reports additions/removals, and identifies relocation candidates only for unique matching SHA-256
and size pairs. A matching hash does not establish who moved a file or why.
