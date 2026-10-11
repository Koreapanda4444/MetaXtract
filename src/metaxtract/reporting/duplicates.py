from __future__ import annotations

from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

from ..case.paths import resolve_source
from ..config import MEBIBYTE
from ..core.files import sha256_file
from ..core.models import require_valid_records


MAX_VISUAL_FILE_BYTES = 64 * MEBIBYTE
MAX_VISUAL_IMAGE_PIXELS = 40_000_000
MAX_VISUAL_IMAGES = 2_000
VISUAL_SUFFIXES = {".bmp", ".gif", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"}


@dataclass(frozen=True)
class ExactDuplicateGroup:
    sha256: str
    file_count: int
    size_bytes: int | None
    reclaimable_bytes: int | None
    paths: tuple[str, ...]


@dataclass(frozen=True)
class VisualMatch:
    first_path: str
    second_path: str
    hamming_distance: int
    first_hash: str
    second_hash: str


@dataclass(frozen=True)
class VisualSkip:
    path: str
    reason: str


def _exact_duplicate_groups(
    records: list[dict[str, Any]],
) -> list[ExactDuplicateGroup]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        grouped[str(record["sha256"])].append(record)

    groups = []
    for sha256, members in grouped.items():
        if len(members) < 2:
            continue
        paths = tuple(sorted((str(item["path"]) for item in members), key=str.casefold))
        sizes = {int(item["size_bytes"]) for item in members}
        size_bytes = next(iter(sizes)) if len(sizes) == 1 else None
        reclaimable = size_bytes * (len(paths) - 1) if size_bytes is not None else None
        groups.append(
            ExactDuplicateGroup(
                sha256=sha256,
                file_count=len(paths),
                size_bytes=size_bytes,
                reclaimable_bytes=reclaimable,
                paths=paths,
            )
        )
    return sorted(groups, key=lambda item: (item.paths[0].casefold(), item.sha256))


def exact_duplicate_groups(
    records: Iterable[dict[str, Any]],
) -> list[ExactDuplicateGroup]:
    return _exact_duplicate_groups(require_valid_records(records))


def _is_visual_candidate(record: dict[str, Any]) -> bool:
    return (
        str(record["mime"]).casefold().startswith("image/")
        or Path(str(record["path"])).suffix.casefold() in VISUAL_SUFFIXES
    )


def _perceptual_hash(path: Path) -> str:
    from PIL import Image, ImageOps

    try:
        with Image.open(path) as image:
            width, height = image.size
            if width < 1 or height < 1 or width * height > MAX_VISUAL_IMAGE_PIXELS:
                raise ValueError("image_pixel_limit")
            image.seek(0)
            grayscale = ImageOps.exif_transpose(image).convert("L")
            pixels = list(grayscale.resize((9, 8), Image.Resampling.LANCZOS).getdata())
    except Image.DecompressionBombError as exc:
        raise ValueError("image_pixel_limit") from exc
    bits = 0
    for row in range(8):
        offset = row * 9
        for column in range(8):
            bits = (bits << 1) | int(pixels[offset + column] > pixels[offset + column + 1])
    return f"{bits:016x}"


def _hamming_distance(first: str, second: str) -> int:
    return (int(first, 16) ^ int(second, 16)).bit_count()


def _visual_hashes(
    records: list[dict[str, Any]],
    files_base: str | Path,
) -> tuple[list[tuple[dict[str, Any], str]], list[VisualSkip]]:
    base_path = Path(files_base).expanduser()
    if base_path.is_symlink():
        raise ValueError("files base cannot be a symbolic link")
    base = base_path.resolve(strict=True)
    if not base.is_dir():
        raise NotADirectoryError(f"files base is not a directory: {files_base}")

    candidates = [record for record in records if _is_visual_candidate(record)]
    if len(candidates) > MAX_VISUAL_IMAGES:
        raise ValueError(f"visual image limit exceeded: more than {MAX_VISUAL_IMAGES} images")

    hashes = []
    skipped = []
    for record in candidates:
        path = str(record["path"])
        try:
            source = resolve_source(base, path)
            source_size = source.stat().st_size
            if source_size > MAX_VISUAL_FILE_BYTES:
                raise ValueError("file_size_limit")
            if source_size != int(record["size_bytes"]):
                raise ValueError("recorded_size_mismatch")
            if sha256_file(source) != str(record["sha256"]):
                raise ValueError("recorded_hash_mismatch")
            hashes.append((record, _perceptual_hash(source)))
        except FileNotFoundError:
            skipped.append(VisualSkip(path, "source_missing"))
        except (OSError, ValueError) as exc:
            skipped.append(VisualSkip(path, str(exc) or type(exc).__name__))
    hashes.sort(key=lambda item: str(item[0]["path"]).casefold())
    skipped.sort(key=lambda item: item.path.casefold())
    return hashes, skipped


def find_visual_matches(
    records: Iterable[dict[str, Any]],
    files_base: str | Path,
    *,
    threshold: int = 8,
) -> tuple[list[VisualMatch], list[VisualSkip]]:
    if not 0 <= threshold <= 64:
        raise ValueError("visual threshold must be between 0 and 64")
    valid_records = require_valid_records(records)
    hashes, skipped = _visual_hashes(valid_records, files_base)
    matches = []
    for index, (first, first_hash) in enumerate(hashes):
        for second, second_hash in hashes[index + 1 :]:
            if first["sha256"] == second["sha256"]:
                continue
            distance = _hamming_distance(first_hash, second_hash)
            if distance <= threshold:
                matches.append(
                    VisualMatch(
                        first_path=str(first["path"]),
                        second_path=str(second["path"]),
                        hamming_distance=distance,
                        first_hash=first_hash,
                        second_hash=second_hash,
                    )
                )
    matches.sort(
        key=lambda item: (
            item.hamming_distance,
            item.first_path.casefold(),
            item.second_path.casefold(),
        )
    )
    return matches, skipped


def duplicate_document(
    records: Iterable[dict[str, Any]],
    *,
    files_base: str | Path | None = None,
    threshold: int = 8,
) -> dict[str, Any]:
    if not 0 <= threshold <= 64:
        raise ValueError("visual threshold must be between 0 and 64")
    valid_records = require_valid_records(records)
    exact_groups = _exact_duplicate_groups(valid_records)
    matches: list[VisualMatch] = []
    skipped: list[VisualSkip] = []
    visual_status = "source_unavailable"
    if files_base is not None:
        matches, skipped = find_visual_matches(
            valid_records,
            files_base,
            threshold=threshold,
        )
        visual_status = "complete"

    return {
        "exact": {
            "group_count": len(exact_groups),
            "duplicate_file_count": sum(item.file_count for item in exact_groups),
            "reclaimable_bytes": sum(item.reclaimable_bytes or 0 for item in exact_groups),
            "groups": [asdict(item) for item in exact_groups],
        },
        "visual": {
            "status": visual_status,
            "threshold": threshold,
            "match_count": len(matches),
            "matches": [asdict(item) for item in matches],
            "skipped": [asdict(item) for item in skipped],
        },
    }
