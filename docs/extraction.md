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

## HEIF and camera RAW

`images.heif` walks HEIF/HEIC/AVIF item information, extent locations, primary-item and
property associations, spatial/color/transform properties and item references. EXIF and XMP
in file extents or `idat` are extracted; contiguous data keeps source offsets, assembled
fragments identify their item instead. Protected/external/unsupported item constructions are
reported without fetching external data. Image codec bitstreams are retained as properties,
not decoded to certify pixels. Nikon type-2 maker-note TIFF directories are also interpreted
by `images.metadata`, with their raw maker-note retained.

`images.raw` dynamically bundles LibRaw 0.22.2 and uses a Rust-backed read-only stream to
inspect supported camera RAW families, including DNG, CR2/CR3, NEF, ARW, RAF, ORF and RW2.
It returns camera/lens/serial/exposure/sensor/GPS/thumbnail and maker-note-derived fields,
alongside source-located raw callback tags. Library-normalized names can differ from original
tags; both remain distinguishable. LibRaw defaults are not proof that a field was present.
No RAW pixel unpacking or filesystem opening is requested.

LibRaw gets cooperative read/progress budgets, a 16 MiB raw-memory setting, and a precharged
working-memory allowance. This is not a complete meter of every internal library allocation
or a preemptive hard timeout. Isolated scan workers remain later work; exhausted budgets and
library failures are surfaced with any observations already obtained.

## Image reconciliation

Rust `metaxtract_core::reconcile_image_metadata` builds provenance-linked canonical claims
without modifying observations. It separates main, sensor, thumbnail, frame, page and icon
variant dimensions, composes primary HEIF transforms into EXIF orientation, checks color
channel compatibility, and retains incompatible alternatives as unresolved conflicts.
Unlinked HEIF metadata is not silently assigned to the primary image. Display dimensions
are available only when the main geometry and any stated orientation have no conflict.

EXIF/ISO timestamps validate calendar fields and retain offset/fraction companions. No local
timezone is assumed, `-00:00` remains unknown, and leap seconds are retained without ordinary
Unix-second projection. Differing precision alone does not establish a timestamp conflict.
Correlated capabilities of the same parser family do not count as independent agreement.
Duplicate observation IDs and mixed sources are rejected. Configurable analysis limits bound
observation count, alternatives per key and claim links; exhaustion reports an error and
does not silently truncate the source observations. Full case/record parity is still pending.

## Audio metadata

`audio.metadata` combines source-located native structures with TagLib 2.3.2 interpretation
through a Rust-owned read-only stream. It reads MP3 ID3v1/v2 frames (including unknown frames),
FLAC STREAMINFO and metadata blocks, Vorbis/Opus headers and comments, WAV/AIFF chunks,
BWF origin and timing fields, APEv2 items, and APE/WavPack technical properties. Repeated tags
are preserved. UTF-8, Latin-1 and unspecified text encodings remain distinct. Compressed and
unsynchronized ID3 packets retain their original bytes for library interpretation. TagLib also
exposes supported M4A/MP4, ASF and DSF tags; native container detail is handled separately.

Library values name the TagLib stream and do not invent source offsets. The adapter retains
partial source observations when a library cannot inspect a damaged file. Metadata success
is not proof that every audio frame decodes. Ogg headers and continuations are bounded; full
page CRC and codec-frame verification are not performed here. Library reads and output are
budgeted, and a 16 MiB working allowance is charged before TagLib. Its internal allocations
and cooperative deadline still require later scanner isolation for a hard memory/time boundary.

## Media containers

The media.structure parser walks MP4/MOV boxes, including 64-bit sizes and movie fragments;
reads movie/track clocks, display matrices, dimensions, language, handlers, sample
descriptions, sample counts/tables and chunk offsets; and retains unknown metadata boxes.
QuickTime without ftyp is supported. Media bytes in mdat are not copied as metadata, and
external data references are never opened.

Matroska/WebM inspection reads EBML, segment/track identities, timing, codecs, video/audio
properties, language, color fields, chapters, tags and attachment payloads. Streaming segments
and clusters with unknown sizes stop at known sibling boundaries. Unknown metadata elements
are retained; large encoded blocks are skipped. AVI inspection reads main and stream headers,
formats, INFO chunks, OpenDML frame counts and index sizes while skipping the media list.

MPEG elementary/program streams expose sequence geometry and rates, pack/system headers,
stream IDs and user data. Transport streams support 188/192/204-byte packets, bounded
multi-packet PAT/PMT assembly, CRC checks, program/elementary PIDs and descriptor packets.
Continuity gaps, scrambled payloads, incomplete PSI and trailing packets remain explicit.
This parser inspects declared container structures; it does not certify every codec frame,
resolve external resources or reconstruct an entire playback timeline. Original source
ranges and assembled-packet provenance remain separate, and all descent/read/output budgets apply.

## Media details and embedded artwork

The media.details capability adds native sample descriptions, AVC/HEVC/AV1 parameter packets,
MPEG-4 descriptors, color/ICC and pixel aspect data, edit lists, chapter timing/track references,
QuickTime keys and typed metadata, cover data, subtitle configuration, Matroska block header
timing, AVI format fields and transport language/subtitle/registration descriptors. ID3 CHAP/
CTOC/APIC and FLAC pictures retain original byte ranges; TagLib complex properties independently
return picture bytes and attributes with library provenance. No attachment or linked artwork
is opened on disk or over a network.

FourCC bytes are preserved as original encoded text. Internal path components escape non-ASCII
and control bytes reversibly, preventing malformed names from breaking the Rust boundary.
Embedded resources are bounded byte observations at this stage; virtual child evidence and
GUI previews come later. Codec configuration is declared data, not a successful decode result.
Extended QuickTime sound layouts and laced Matroska per-frame boundaries remain explicitly
uninterpreted for the in-process demuxer rather than guessed.

## In-process media inspection and decoding

media.inspect and media.decode link FFmpeg 9.0.2 shared libraries through Rust-backed custom
I/O. Programs, network support, protocols, encoders, GPL/nonfree components and automatic
system dependency selection are disabled. External-resource opening is denied.

Inspection extracts container/track identities, clocks, rates, language/tags, color fields,
codec private data, programs, chapters and attached pictures. Library values identify their
track/chapter/program without invented original offsets. Source-located media.structure and
media.details observations remain available separately.

Decoding returns one frame per supported audio/video track: audio samples with format/layout,
or RGB24 image pixels with the longest preview side limited to 256. This does not validate every
frame. Common AVC/HEVC, MPEG, VP8/VP9, JPEG/PNG, raw video, MP3/AAC/AC3, FLAC/Vorbis/Opus,
ALAC/APE/WavPack and PCM software decoders are enabled. A codec/profile can still be unavailable;
AV1 without an enabled software backend is unsupported for decoding. Inspection results and
explicit decode diagnostics remain available for unsupported codecs.

Reads, packet traversal, decoder frame buffers, preview copies and output are budgeted.
A 32 MiB working allowance and a 64 MiB maximum individual FFmpeg allocation complement
these checks. They do not meter all library allocations or provide a forced timeout;
the later scanner isolation boundary remains necessary.

## PDF metadata and revisions

pdf.metadata statically links reviewed qpdf 12.4.2 and libjpeg-turbo 3.1.4.1. Rust-backed custom
input permits read-only parsing without qpdf programs or external resources. It preserves Info
strings and uninterpreted string bytes, custom fields, catalog attributes, XMP packets, inherited
page geometry, Unicode attachment names, stream dictionaries, associations and decoded payloads.

A bounded lexical reader walks the actual final startxref, classic tables, cross-reference
streams, hybrid supplements and Prev chain. It retains original dictionary/row bytes and offsets.
EOF-like text inside streams does not count as a revision. Deleted generations cannot revive older
slot mappings. Compressed cross-reference/object streams are supported. Library-resolved fields
identify their object and do not invent byte ranges. An indirect raw xref-stream length is explicitly
reported as requiring library resolution; metadata parsing still uses qpdf's full resolver.

Unknown stream filters, malformed trees and budget exhaustion preserve earlier observations
and report incomplete extraction. Empty-password encryption can be opened by qpdf; protected
documents report password_required and remain partial. Password entry is not yet exposed.

The adapter charges a 32 MiB working allowance, checks read/traversal/output limits and inserts
parser-local budget checks before qpdf's internal buffer and string growth, including object-stream
decompression. Warnings are limited to 100 per extraction and charged before retention. The adapter
does not enable qpdf's eager page-repair pass; page traversal and inherited geometry use the bounded
read-only adapter. These checks do not meter every allocation or provide preemptive worker isolation.

## PDF security structures

pdf.security includes pdf.metadata results and inventories every current cross-reference object,
including unreachable dictionaries and compressed objects. Direct dictionaries/arrays obey depth
and entry limits; indirect edges retain identities and are inspected once through the object table,
so ordinary page-parent and action-reference cycles do not cause unbounded descent. Unknown
keys, strings and string bytes, stream filters, object-stream identities and original object offsets
remain available, with original dictionary bytes for current uncompressed dictionaries/streams.
Compression and embedding markers are separate from active-content features; neither implies
maliciousness. Historical replaced/deleted objects are not reconstructed.

Encryption inspection preserves raw Standard/security-handler and crypt-filter dictionaries even
when the document requires a password. Empty-password RC4/AES documents additionally expose
library-resolved methods and permission flags. No password or derived encryption key is returned.
Password entry remains pending.

Signature inspection retains dictionaries, certificate/signature bytes, signing claims and
ByteRange values. It checks integer counts, ordering, source bounds, final covered offset,
unsigned tail size and whether a two-range exclusion matches the source offset and bytes of the
Contents hex string. Compressed or indirect Contents report the offset check as unavailable.
These are structural facts. cryptographic_verification is always not_performed: CMS digests,
certificate trust, revocation and signer authenticity are not validated.

Revision output follows actual cross-reference sections, not EOF string counts. Forward Prev
links are retained for linearized PDFs rather than rejected as out of order. The incremental count
subtracts forward xref links; it is a structural section count, not complete historical revision
reconstruction or linearization certification. Non-whitespace data after the final EOF is explicit.

JavaScript strings/streams, launch/URI/remote actions, OpenAction/AA, XFA, RichMedia and embedded
file markers are preserved as features. Only selected JavaScript streams are decoded; external
streams are denied. Actions, scripts, URLs and filenames are never executed or opened. C++
returns observed structures; severity and investigation policy belong to Rust.
