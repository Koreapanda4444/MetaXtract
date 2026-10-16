# Conformance corpus

[English](README.md) | [한국어](README_KR.md) | [Project README](../README.md)

This directory replaces a generic root test tree for migration-critical verification. The corpus
contains deterministic byte payloads and stable output projections. It never contains private
evidence or files copied from users.

The checks require Rust 1.88, CMake 3.24 or newer, a C++20 compiler, and the Python reference.
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

The runner compares Python baseline records and path behavior, then sends fixture bytes through
Rust, C++, and the Rust-owned memory/file callback. It checks byte ranges, short reads at EOF,
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
Full PNG/PDF/other metadata record parity is still pending. The result reports this
explicitly instead of treating the Python-only records as native extraction coverage. To check
only the frozen Python baseline without native build tools, use `--python-only`.

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

The document corpus adds independently generated Office/ODF ZIP packages, Unicode properties, relationship/revision metadata, ambiguous members, compression/CRC/ZIP64/data descriptor cases, and resource limits.

The OLE corpus independently serializes CFB v3/v4, OLEPS, Word FIB, BIFF8, PowerPoint and native embedded data. It exercises Unicode, fragmented/mini streams, orphan records, unknown property types/code pages, corrupted sector and directory links, impossible string lengths, encryption and shared resource budgets.

The email corpus exercises EML headers/routes, RFC2047 Unicode words, RFC2231 filenames and continuations, MIME boundaries/digests, HTML and binary bodies, identity/decoded nested messages, duplicate controls, invalid transfer/character encodings and shared budgets. Claims and verification states are checked separately.
