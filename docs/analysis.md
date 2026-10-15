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

## Structured search and saved filters

`search::parse(scan_id, text)` accepts AND-separated terms, quoted values and leading `-`
negation. Examples: `mime=image/jpeg size>=1024 claim.metadata.author="Alice Smith"`,
`tag=review raw:exif`, and `claim.metadata.created>=unix:1704067200`. `*` checks field existence.
The typed `SearchExpression` API additionally supports nested OR/AND/NOT groups. Text matching
is case-sensitive. Numeric claims use their stored type, integer sizes retain the full u64 range,
and timestamp ranges require explicit UTC epoch seconds. `claim.key!=value` excludes matching
values, including when other alternatives exist. Free text covers paths, MIME, claims and notes.
Raw namespaces are searchable; encoded binary payloads are not treated as prose.

`CaseWorkspace::search_records`, `count_search`, and `search_facets` execute bound SQL with
existing indexes and keyset paging, without loading complete cases. Contains predicates may
scan matching index candidates. Facets count distinct records and expose pagination truncation.
Cursors bind the full query, case and revision. Save the typed query as `FilterQuery::Structured`;
existing legacy and record filters remain readable. These newer filters require this engine when
opening a case. Writes remain covered by the case audit chain.

## Previewable redaction profiles

`redaction::preview` creates a separate metadata derivative and a change manifest without changing
the case or source bytes. Profiles select canonical-key prefixes, raw namespaces/names, privacy
fields or all metadata, with removal or explicit mask text. Dependency expansion covers linked
observations, competing claims for the same field and conflict references, preventing a selected
value from surviving in another linked interpretation. Conflicting overlapping actions are rejected.
Masks are labelled as generated derivatives, not original parser output.

The privacy profile strips encoded payloads and unstructured diagnostics/container context. File
paths, digests and filesystem dates each have explicit options; path hiding requires a caller-
supplied safe alias. The preview reports unselected fields for review. This is field-aware metadata
redaction, not a sanitizer for original file content, and unselected free text may retain sensitive
information. Export the derivative record, not the preview's original-value comparison.

## Selected exports

`export::export_records` streams filtered case pages into JSONL or CSV. JSONL preserves the typed
record; CSV contains path, MIME, full-range size, status, SHA-256, all claims as JSON, and diagnostic
count. CSV text cells are quoted and spreadsheet formula prefixes are neutralized. An optional
redaction profile exports only its derivative, with generated aliases when paths are hidden.

`export_original_evidence` creates a selected-evidence ZIP with per-record metadata, original bytes
and a manifest. The caller supplies safely resolved opened sources, including virtual members.
Every original must match its recorded size/SHA-256. This format is a selected evidence package,
not a complete portable case snapshot; original embedded metadata is retained. It intentionally
does not accept a redaction profile. A separate byte sanitizer would be needed to sanitize originals.

Both workflows verify the case chain/state, bind a revision and audit head, enforce selection/byte
budgets, and return an output SHA-256 receipt. They fail if the case changes. Output publication
uses a same-directory temporary file and does not overwrite existing files; errors leave no partial
destination. Audit consistency alone is not independent authentication of the case's author.

## Investigation reports

`reporting::build_report` verifies case state, records the scan history and audit head, and derives
normalization, validation, field lineage, findings, timelines, duplicates, locations, clusters, tracks
and an optional baseline comparison. Scope includes selected/total counts, extraction status and
decoder availability. Invalid coordinates remain findings and are excluded from spatial calculations
with explicit coverage issues. Original observations and stored claims remain distinguishable from
new analysis; binary fields are displayed by length/SHA-256, with originals retained in the case.

Optional verified content fingerprints enable near-content comparison. Absent or partial fingerprints
and missing native previews are reported as coverage gaps. Visual reporting uses decoded preview
orientation as supplied; callers can use the explicit orientation API for other comparisons. Reports
do not include original evidence bytes. Generated time is supplied by the caller for reproducibility.

`write_report` writes standalone HTML or JSON atomically. HTML escapes all evidence text, uses a
restrictive Content Security Policy and fetches no external assets. Section labels support English,
Korean or both; raw evidence and rule explanations retain their original language. The returned
receipt includes output length/SHA-256 and, optionally, an Ed25519 signature over `signed_manifest`.
Verify that manifest with `verify_manifest` and an independently trusted public key. Preserve the
receipt beside the report when authentication is required; a digest inside its own report is not an
independent signature. An optional trusted case anchor also verifies its recorded chain prefix.

Redacted reports omit the original search expression and scan-source path, apply the profile before
analysis, and preserve derivative labels. Titles, analyst names, track group labels and unselected
free text remain caller-provided content requiring review. No new Rust CLI/GUI is introduced here.
The library APIs are the integration surface for the next application stages.

```rust,ignore
let query = metaxtract_analysis::search::parse(&scan_id, "mime=image/jpeg size>=1024")?;
let options = metaxtract_analysis::reporting::ReportOptions::new(
    "Metadata investigation / 메타데이터 조사", "Analyst", generated_unix_seconds,
);
let report = metaxtract_analysis::reporting::build_report(
    &case, &query, &options, None, None, Default::default(),
)?;
let receipt = metaxtract_analysis::reporting::write_report(
    &report, "report.html", metaxtract_analysis::reporting::ReportFormat::Html,
    None, Default::default(),
)?;
```
