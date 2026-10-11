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
