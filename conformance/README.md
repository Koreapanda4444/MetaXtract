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
The object-stream budget case checks qpdf's internal decoded-buffer guard.
