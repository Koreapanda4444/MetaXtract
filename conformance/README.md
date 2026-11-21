# Conformance corpus

C2PA coverage includes unchanged C/CA JPEG fixtures from the official SDK at
`6b5b65343232de3cc50a1ca4254c4a27bfcafb26`, with SHA-256/Git blob identities and the
original Apache notice. The CA sample exercises a multi-segment APP11 store. Independent
synthetic tests cover missing/duplicate/noncontiguous fragments, PNG CRCs, JUMBF bounds,
unknown UUIDs and resource ceilings. Application checks retain raw packet links and explicitly
scoped verification states; extraction alone is not a C2PA compliance certification.

Independent OpenSSL RSA-PSS and Python hash calculations verify the two SDK inputs. Separate
OpenSSL-signed v2 PNG fixtures cover a valid binding and a maliciously broad pixel exclusion.
Native tests cover all supported COSE algorithms and distinguish changed asset, assertion and
claim bytes. Certificate trust, timestamps and full-profile compliance are never inferred.

Extended JPEG XMP cases now verify linked, out-of-order fragment assembly and its MD5 packet
identifier, missing/overlapping fragments, invalid digests, orphan packets and the member-size
ceiling. Original fragment bytes and ordinary XMP observations remain available on failure.
An MD5 identifier is a format consistency check and does not authenticate a photographer or file.

`python conformance/differential.py --dist dist --mediainfo /usr/bin/mediainfo` compares the
packaged CLI with separately installed ExifTool and MediaInfo. `--metaxtract PATH` can instead
select a development CLI. The gate records reference versions/input hashes, compares common
camera fields and audio sample rates/channels, checks that source bytes survive unchanged,
and fails on unavailable tools, absent reference fields or mismatches. It normalizes padding
and the schema's lossless numeric representations, not conflicting values. Linux installation
CI runs this gate; these tools and this Python checker are never runtime dependencies.

The camera capture corpus adds nine unchanged libexif device metadata samples, including Canon,
Fujifilm, Nikon, Olympus, Sanyo, Pentax and Casio variants and both TIFF byte orders. Their
independent upstream `.parsed` Make/Model expectations, exact Git blob identities and SHA-256
digests are retained. The upstream metadata-only JPEG fragments intentionally produce a partial
malformed-image result while retaining their independently known EXIF fields. Captured-only
validation checks their bytes; normal debug/release and
sanitizer runs also extract them and include them in mutations. Upstream LGPL notices are in
`licenses/libexif-COPYING.txt`. These are metadata captures, not authenticated camera photographs;
actual encoded media/document samples and generated HEIF fixtures remain separately identified.
Two unchanged AndroidX LGE Nexus 5 and Samsung SM-N900S smartphone JPEGs additionally check
independent upstream manufacturer/model/capture timestamps (Apache-2.0; see androidx-NOTICE.txt).
Coverage is bounded and does not promise every smartphone, device or format variant.

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

This directory replaces a generic root test tree for migration-critical verification. The corpus
contains deterministic byte payloads and stable output projections. It never contains private
evidence or files copied from users.

The checks require Rust 1.88, CMake 3.24 or newer, a C++20 compiler and Python 3.11+
for the standard-library-only harness. No Python application or extraction package is installed.
Visual Studio 2026 on Windows requires CMake 4.2 or newer (CI uses 4.2.3). Run from the
repository root:

```bash
python conformance/run.py
python conformance/run.py --release
python conformance/validate.py --cargo
cmake --preset dev
cmake --build --preset dev --config Debug
ctest --preset dev -C Debug
```

Volatile filesystem fields are removed before comparison. The corpus establishes compatibility
for existing behavior; deeper fields introduced by the native engine require format-spec or
multi-parser evidence and are not constrained to Python's shallow output.

The runner validates captured input hashes, then sends fixture bytes and retained path cases
through Rust, C++ and the Rust-owned memory/file callback. It checks byte ranges, short reads at EOF,
overflow offsets, read ceilings, truncated files, error mapping, schema invariants, and every
parser resource budget. The fixture parser is enabled only by the explicit Cargo `conformance`
feature and is absent from ordinary production builds.

Native image structure fields are checked against generated fixtures, including endian variants,
CRC errors, directory cycles, truncation, partial results, and source-byte provenance.
Image metadata fixtures additionally cover EXIF subdirectories, unknown signed/float tags,
XMP namespace/entity/depth handling, IPTC, multipart ICC, compressed PNG metadata,
thumbnail/comment payloads and decompression ceilings. Decoded fields must identify their
source packet and must not claim original-file coordinates.
HEIF/RAW fixtures include an encoded AVIF, a generated DNG, item/property/extent bounds,
maker-note tags, external-reference refusal and library working-memory limits.
The migration acceptance gate runs eight captured Python inputs through the actual application:
plain text, RGB and RGBA PNG, animated PNG, EXIF/GPS JPEG, ordinary and password-protected PDF,
and DOCX. It compares path, MIME type, size, SHA-256 and every captured metadata field while
allowing additional native observations. Unknown Unicode metadata and orientation claims must
survive. `--acceptance-only` runs the application contracts and this gate; `--cli-executable PATH`
can select an independently packaged CLI. `--captured-only` validates the retained data without
native build tools. The retired application source is preserved in Git history; expected data
retains the original source-tree identity and live comparison evidence from the acceptance commit.

The same gate imports genuine Python case/JSONL/signed-bundle artifacts, retains original record
fields, notes, annotations and saved-filter semantics, and checks analysis of imported GPS.
Original-file verification, selected evidence integrity, deterministic repeated exports, snapshot
reopening, cache purging and production diagnostics use public application commands. These checks
are bounded acceptance evidence, not exhaustive equivalence across every format. CLI syntax and
case schemas intentionally change. Preview decoding remains bounded; visual hashes use the native
algorithm and are not promised to match Pillow resampling bit for bit. The captured source-tree
identity and original artifacts remain in the expected data for future replay.

CI runs Linux, Windows, and macOS builds and both debug/release contract checks. Linux additionally
runs AddressSanitizer and UndefinedBehaviorSanitizer on the native core and the generated FFI.
Rust policy regression checks also run with `cargo test --workspace --locked`.

Each debug, release and sanitizer contract run also replays `corpus/fuzz-seeds.json` and
256 reproducible mutations of the existing format corpus. Retained malformed seeds marked `*`
exercise every production parser. Truncation, bit flips, hostile lengths and trailing data run
with ordinary, tiny-budget and short-read policies. The Rust bridge rejects internal failures,
budget overruns, fabricated source bytes and missing decoded provenance. Reproduce a campaign
with `python conformance/run.py --fuzz-seed 0x4d58 --fuzz-cases 256`; increase the count up to
10000 for a longer bounded campaign. New minimized failures belong in the same retained corpus,
with their parser and original bytes, before the associated fix is accepted. This deterministic
mutation campaign does not claim exhaustive parser coverage.
Retained regressions include archive error reporting after a read-budget failure, rejection
before archive initialization with an unusable allocation budget, and malformed PDF Flate
cross-reference data. These inputs must produce bounded, recoverable errors and retain earlier fields.

Archive fixtures also check ZIP/TAR and gzip/bzip2/xz filters, member and allocation ceilings,
compressed PAX headers, and skipped special members. CLI checks nest stored ZIPs to verify that
expansion budgets accumulate across the entire tree and that configurable limits retain partial
observations instead of silently dropping evidence.

For the rules these checks protect, see [architecture](../docs/architecture.md),
[the language boundary](../docs/ffi.md), and [parser resource limits](../docs/resource-limits.md).

Audio fixtures cover all eight planned audio families, original and library fields, unknown
ID3 data, Vorbis/Opus comments, chunk/frame bounds and read/observation/allocation limits.
Named TagLib source fixtures retain upstream licensing and exact-revision provenance in the corpus.

Media fixtures cover every planned container family, fragmented/large BMFF boxes, streaming
EBML boundaries, AVI stream headers, MPEG sequence data, transport alignment/PSI CRC,
malformed extents and depth/entry/read ceilings.

Detail fixtures check codec records, title/chapter/language values, original and library picture
bytes, attachments, non-ASCII FourCC transport, timestamp markers and malformed payload ranges.

In-process media fixtures check real MP3/FLAC/Opus frames, declared PCM samples, known PNG
preview pixels, AVI frames, scaling, unsupported codecs, disabled external playlist protocols,
and read/allocation/output limits. Native builds also require Python 3.11+, GNU make and a shell.
Windows needs MSVC and MSYS2 bash/make in C:/msys64, or METAXTRACT_MSYS_ROOT.

PDF fixtures declare known Info/XMP strings, page geometry, Unicode names and attachment
bytes; exercise actual table/stream references, compressed objects and incremental changes;
and check invalid trees, unsupported filters, partial results and each resource ceiling.
The object-stream budget case checks qpdf's internal decoded-buffer guard. A malformed
cross-reference fixture checks the 100-warning retention ceiling without page repair.

PDF security cases use deliberately unauthenticated structural signature placeholders to check
valid, missing, negative, overlapping and out-of-bounds ByteRanges, incorrect exclusion gaps,
unsigned fields and post-signing updates. They check unknown/orphan objects, reference cycles,
compressed JavaScript, external-stream refusal and every resource ceiling. Four unchanged
QPDF encryption samples at the pinned source revision exercise RC4, AES-128 and AES-256,
empty/password-required inputs and linearized references. Their Apache license and original
NOTICE are in licenses/qpdf-Apache-2.0.txt and licenses/qpdf-NOTICE.md.

Native compilation has a separate 1800-second ceiling. The protocol process keeps its
300-second runtime ceiling; parser requests retain their own tighter resource budgets.

CI checks debug extraction contracts before production/release validation. Reviewed FFmpeg build
outputs are cached by platform and source/build-policy hash; runtime libraries still undergo
configuration and relative-path checks. Standalone MSVC builds use the same CRT as Cargo.

Each native CI job also reuses CMake outputs across its sequential Cargo invocations.
The directory is private to that workflow job, with separate configurations for debug/release
profiles, target/ABI flags and sanitizers. Every Cargo build gets its own immutable installed
snapshot, so changing the fixture setting cannot replace a previously linked production library.
Unchanged generated archive sources keep their timestamps. This avoids rebuilding
the same vendored libraries for every Cargo feature graph. It is not a persistent developer cache
or a directory for concurrent Cargo invocations; ordinary local builds retain Cargo's own layout.
CI runs are grouped by commit so pushing a later stage does not cancel verification of an earlier one.

The document corpus adds independently generated Office/ODF ZIP packages, Unicode properties, relationship/revision metadata, ambiguous members, compression/CRC/ZIP64/data descriptor cases, and resource limits.

The OLE corpus independently serializes CFB v3/v4, OLEPS, Word FIB, BIFF8, PowerPoint and native embedded data. It exercises Unicode, fragmented/mini streams, orphan records, unknown property types/code pages, corrupted sector and directory links, impossible string lengths, encryption and shared resource budgets.

The email corpus exercises EML headers/routes, RFC2047 Unicode words, RFC2231 filenames and continuations, MIME boundaries/digests, HTML and binary bodies, identity/decoded nested messages, duplicate controls, invalid transfer/character encodings and shared budgets. Claims and verification states are checked separately.
