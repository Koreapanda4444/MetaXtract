# Native extraction coverage

[English](extraction.md) | [한국어](extraction_KR.md) | [Project README](../README.md)

## Content identification

The Rust `metaxtract_native::detect_content` API calls the C++ signature detector through
bounded read-only input. It reads at most the first 64 KiB and returns all recognized candidates,
including their MIME, container, source byte range, and detection state.

| State | Meaning |
|---|---|
| Signature | Recognized bytes; structural validity is not established |
| HeaderValidated | The stated header checks passed; this does not validate the entire file |
| Truncated | The recognized header or its declared extent exceeds the available input |
| Invalid | Recognized signature with contradictory header fields |

The detector recognizes JPEG, PNG, TIFF/BigTIFF/CR2, WebP, GIF, BMP, ICO/CUR, HEIF/HEIC/AVIF,
RIFF audio/video, AIFF, FLAC, Ogg, ID3/MPEG audio, APE, WavPack, ISO BMFF/QuickTime, EBML,
MPEG program/transport streams, PDF, ZIP, and OLE signatures. ZIP package subtypes, Ogg codecs,
EBML DocType, and individual metadata records require deeper parsers. ID3 alone does not prove
the following data is MP3 audio.

If no signature is recognized, the detector returns an empty candidate list. Read failures and budget exhaustion
produce errors rather than an apparently successful unknown result. A signature at a nonzero
offset and multiple candidates are preserved; detection does not silently select one truth.

See [resource limits](resource-limits.md) for budgets and
[conformance](../conformance/README.md) for normal, truncated, contradictory, and mixed-signature checks.

## Type conflicts

Rust's `metaxtract_core::assess_content` compares candidates with an extension, declared MIME,
and declared container. It reports mismatches, multiple signatures, invalid/truncated headers,
unrecognized content, and unverified subtypes while retaining the original candidates.
Case and common MIME aliases are normalized. A DOCX-like extension over ZIP, or WebM over EBML,
does not prove the package subtype; it remains unverified until a deeper parser confirms it.
An ID3 wrapper similarly does not establish the following audio format. Generic binary MIME
does not create a spurious conflict.

## Image structures

The native `images.structure` parser extracts JPEG frame/scan/segment fields, PNG chunk fields
with CRC and ordering checks, classic TIFF and BigTIFF directories, WebP headers/animation
geometry, GIF frames/control blocks, BMP DIB fields, and ICO/CUR directories and embedded images.
TIFF observations retain unknown tags and original values. Entry offsets and embedded image
ranges are checked against their enclosing source, and directory cycles are rejected.

`metaxtract_native::extract_parser` returns typed values, original bytes, source ranges, budget
statistics, and an optional failure. Observations read before a recoverable failure remain available.
Canonical conversion retains the original bytes separately from the interpreted value.
These are structural and metadata checks; compressed pixels are not decoded to certify image validity.

## Image metadata

`images.metadata` adds nested EXIF/GPS/interoperability/SubIFD directories, signed and floating
TIFF values, original unknown tags, JPEG comments/JFIF thumbnails/Photoshop resources,
IPTC datasets, namespace-aware XMP fields, and ICC headers/tags. JPEG ICC fragments are
ordered and checked for missing/duplicate parts. PNG supports EXIF, text, compressed text,
international text/XMP, compressed ICC, physical dimensions and modification time;
WebP adds EXIF/XMP/ICC/animation metadata, and GIF preserves comments/application blocks.
The original XML/profile/dataset and unknown payloads remain available.

Pugixml parses XMP without resolving custom or external entities. Zlib decompresses metadata
in bounded chunks with allocation and decoded-byte accounting. XML traversal enforces depth,
entry and deadline budgets. Decoded fields have packet/stream provenance and no fabricated
source offsets; direct fields retain their exact encoded source bytes. Extended JPEG XMP
fragments are preserved with an explicit assembly limitation. Image pixels and arbitrary
Photoshop resources are not decoded by this parser.
