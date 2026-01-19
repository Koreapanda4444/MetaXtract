from __future__ import annotations

import json
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal, Optional

import extract_common
import utils


SanitizeMode = Literal["redact", "minimal"]


@dataclass(frozen=True)
class SanitizeFileLog:
    ok: bool
    input_path: str
    output_path: str
    mode: str
    actions: list[str]
    sha256_before: Optional[str]
    sha256_after: Optional[str]
    error: Optional[str] = None


@dataclass(frozen=True)
class SanitizeSummary:
    processed: int
    copied: int
    skipped: int
    errors: int


def _write_sidecar_log(output_path: Path, payload: dict[str, Any]) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8", newline="\n")


def _ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def _sanitize_image_jpeg(src: Path, dst: Path, mode: SanitizeMode) -> list[str]:
    try:
        from PIL import Image
    except Exception as e:
        raise utils.ProcessingError(
            "Pillow가 설치되어 있지 않아 이미지 sanitize를 수행할 수 없습니다.",
            exit_code=utils.ExitCodes.FAILURE,
            cause=e,
        )

    actions: list[str] = []

    with Image.open(src) as img:
        icc_profile = img.info.get("icc_profile")

        exif_bytes: bytes = b""
        try:
            exif_obj = img.getexif()
        except Exception:
            exif_obj = None

        if mode == "minimal":
            if exif_obj and len(exif_obj):
                actions.append("exif: stripped")
            exif_bytes = b""
        else:
            # redact: GPS/Author/Software 등의 흔한 흔적만 제거
            if exif_obj and len(exif_obj):
                removed_any = False
                # 34853=GPSInfo, 315=Artist, 305=Software
                for tag_id, label in [(34853, "exif.GPSInfo"), (315, "exif.Artist"), (305, "exif.Software")]:
                    try:
                        if tag_id in exif_obj:
                            del exif_obj[tag_id]
                            actions.append(f"{label}: removed")
                            removed_any = True
                    except Exception:
                        pass
                if removed_any:
                    try:
                        exif_bytes = exif_obj.tobytes()
                    except Exception:
                        exif_bytes = b""
                else:
                    try:
                        exif_bytes = exif_obj.tobytes()
                    except Exception:
                        exif_bytes = b""
            else:
                exif_bytes = b""

        _ensure_parent(dst)
        save_kwargs: dict[str, Any] = {
            "format": "JPEG",
            "optimize": True,
            "quality": 95,
        }
        if icc_profile:
            save_kwargs["icc_profile"] = icc_profile
        if exif_bytes:
            save_kwargs["exif"] = exif_bytes

        img.save(dst, **save_kwargs)

    if not actions:
        actions.append("image: no-op")

    return actions


def _sanitize_image_png(src: Path, dst: Path, mode: SanitizeMode) -> list[str]:
    try:
        from PIL import Image
    except Exception as e:
        raise utils.ProcessingError(
            "Pillow가 설치되어 있지 않아 이미지 sanitize를 수행할 수 없습니다.",
            exit_code=utils.ExitCodes.FAILURE,
            cause=e,
        )

    actions: list[str] = []

    with Image.open(src) as img:
        icc_profile = img.info.get("icc_profile")

        # Pillow는 pnginfo를 넘기지 않으면 텍스트 청크를 기본적으로 다시 쓰지 않지만,
        # 안전하게 최소/레닥트 모두 메타를 전달하지 않는 방식으로 저장합니다.
        if mode in {"minimal", "redact"}:
            actions.append("png: metadata not preserved")

        _ensure_parent(dst)
        save_kwargs: dict[str, Any] = {"format": "PNG"}
        if icc_profile:
            save_kwargs["icc_profile"] = icc_profile
        img.save(dst, **save_kwargs)

    return actions


def _copy_as_is(src: Path, dst: Path) -> list[str]:
    _ensure_parent(dst)
    shutil.copy2(src, dst)
    return ["copied: unsupported format"]


def sanitize_file(src: Path, dst: Path, *, mode: SanitizeMode) -> SanitizeFileLog:
    sha_before = None
    sha_after = None
    actions: list[str] = []

    try:
        sha_before = extract_common.compute_hash(src, "sha256")

        ext = src.suffix.lower()
        if ext in {".jpg", ".jpeg"}:
            actions = _sanitize_image_jpeg(src, dst, mode)
        elif ext == ".png":
            actions = _sanitize_image_png(src, dst, mode)
        else:
            actions = _copy_as_is(src, dst)

        sha_after = extract_common.compute_hash(dst, "sha256")

        return SanitizeFileLog(
            ok=True,
            input_path=str(src).replace("\\", "/"),
            output_path=str(dst).replace("\\", "/"),
            mode=str(mode),
            actions=actions,
            sha256_before=sha_before,
            sha256_after=sha_after,
            error=None,
        )
    except Exception as e:
        return SanitizeFileLog(
            ok=False,
            input_path=str(src).replace("\\", "/"),
            output_path=str(dst).replace("\\", "/"),
            mode=str(mode),
            actions=actions,
            sha256_before=sha_before,
            sha256_after=sha_after,
            error=str(e),
        )


def sanitize_path(input_path: str | Path, outdir: str | Path, *, mode: SanitizeMode = "redact") -> SanitizeSummary:
    src_root = Path(input_path)
    dst_root = Path(outdir)

    if not src_root.exists():
        raise FileNotFoundError(f"경로가 존재하지 않습니다: {src_root}")

    processed = 0
    copied = 0
    skipped = 0
    errors = 0

    def _dst_for(src_file: Path) -> Path:
        if src_root.is_dir():
            rel = src_file.relative_to(src_root)
            return dst_root / rel
        return dst_root / src_file.name

    def _log_for(src_file: Path) -> Path:
        if src_root.is_dir():
            rel = src_file.relative_to(src_root)
        else:
            rel = Path(src_file.name)
        log_base = dst_root / "__metaxtract_logs" / "sanitize"
        return (log_base / rel).with_name(rel.name + ".sanitize.json")

    files: list[Path] = []
    if src_root.is_file():
        files = [src_root]
    else:
        for p in src_root.rglob("*"):
            if p.is_file():
                files.append(p)

    for src in files:
        dst = _dst_for(src)
        log = sanitize_file(src, dst, mode=mode)

        payload = {
            "type": "sanitize",
            "ok": log.ok,
            "mode": log.mode,
            "input_path": log.input_path,
            "output_path": log.output_path,
            "actions": log.actions,
            "sha256_before": log.sha256_before,
            "sha256_after": log.sha256_after,
            "error": log.error,
        }
        _write_sidecar_log(_log_for(src), payload)

        if not log.ok:
            errors += 1
            continue

        ext = src.suffix.lower()
        if ext in {".jpg", ".jpeg", ".png"}:
            processed += 1
        else:
            copied += 1

    return SanitizeSummary(processed=processed, copied=copied, skipped=skipped, errors=errors)
