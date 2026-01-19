from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Optional


@dataclass(frozen=True)
class VideoExtractResult:
    ok: bool
    data: dict[str, Any]
    error_code: Optional[str] = None
    message_short: Optional[str] = None


def _short_message(text: Any, *, limit: int = 160) -> str:
    try:
        msg = str(text).strip()
    except Exception:
        msg = ""
    if not msg:
        msg = "error"
    if len(msg) > limit:
        msg = msg[: max(0, limit - 1)] + "…"
    return msg


_EPOCH_1904 = datetime(1904, 1, 1, tzinfo=timezone.utc)


def _u32(b: bytes) -> int:
    return int.from_bytes(b, "big", signed=False)


def _u64(b: bytes) -> int:
    return int.from_bytes(b, "big", signed=False)


def _read_exact(f, n: int) -> Optional[bytes]:
    try:
        b = f.read(n)
        if b is None or len(b) != n:
            return None
        return b
    except Exception:
        return None


def _read_box_header(f) -> Optional[tuple[int, str, int]]:
    h = _read_exact(f, 8)
    if not h:
        return None
    size = _u32(h[0:4])
    typ = h[4:8].decode("latin-1", errors="replace")
    header_size = 8
    if size == 1:
        ext = _read_exact(f, 8)
        if not ext:
            return None
        size = _u64(ext)
        header_size = 16
    return size, typ, header_size


def _iter_boxes(f, start: int, end: int):
    pos = start
    while pos + 8 <= end:
        try:
            f.seek(pos)
        except Exception:
            return
        hdr = _read_box_header(f)
        if not hdr:
            return
        size, typ, header_size = hdr
        if size < header_size:
            return
        box_end = pos + size
        if box_end > end:
            return
        yield pos, size, typ, header_size
        pos = box_end


def _find_moov(f, file_size: int) -> Optional[tuple[int, int, int]]:
    for pos, size, typ, header_size in _iter_boxes(f, 0, file_size):
        if typ == "moov":
            return pos, size, header_size
    return None


def _parse_mvhd(f, start: int, end: int) -> tuple[Optional[float], Optional[str]]:
    for pos, size, typ, header_size in _iter_boxes(f, start, end):
        if typ != "mvhd":
            continue
        try:
            f.seek(pos + header_size)
        except Exception:
            return None, None
        header = _read_exact(f, 4)
        if not header:
            return None, None
        version = header[0]
        if version == 1:
            body = _read_exact(f, 28)
            if not body:
                return None, None
            creation = _u64(body[0:8])
            timescale = _u32(body[16:20])
            duration = _u64(body[20:28])
        else:
            body = _read_exact(f, 16)
            if not body:
                return None, None
            creation = _u32(body[0:4])
            timescale = _u32(body[8:12])
            duration = _u32(body[12:16])
        created_iso = None
        if creation:
            try:
                created_iso = (_EPOCH_1904 + timedelta(seconds=int(creation))).isoformat()
            except Exception:
                created_iso = None
        dur_sec = None
        if timescale and duration is not None:
            try:
                dur_sec = float(duration) / float(timescale)
            except Exception:
                dur_sec = None
        return dur_sec, created_iso
    return None, None


def _parse_hdlr_handler(f, start: int, end: int) -> Optional[str]:
    for pos, size, typ, header_size in _iter_boxes(f, start, end):
        if typ != "hdlr":
            continue
        try:
            f.seek(pos + header_size)
        except Exception:
            return None
        b = _read_exact(f, 12)
        if not b:
            return None
        handler = b[8:12].decode("latin-1", errors="replace")
        return handler
    return None


def _parse_tkhd_dims(f, start: int, end: int) -> tuple[Optional[int], Optional[int]]:
    for pos, size, typ, header_size in _iter_boxes(f, start, end):
        if typ != "tkhd":
            continue
        try:
            f.seek(pos + header_size)
        except Exception:
            return None, None
        full = _read_exact(f, 4)
        if not full:
            return None, None
        version = full[0]
        if version == 1:
            base_len = 4 + 8 + 8 + 4 + 4 + 8
        else:
            base_len = 4 + 4 + 4 + 4 + 4 + 4
        skip = base_len + 8
        try:
            f.seek(pos + header_size + skip)
        except Exception:
            return None, None
        rest = _read_exact(f, 2 + 2 + 2 + 2 + 36 + 4 + 4)
        if not rest:
            return None, None
        w = _u32(rest[-8:-4])
        h = _u32(rest[-4:])
        width = int(w >> 16) if w else None
        height = int(h >> 16) if h else None
        return width, height
    return None, None


def _parse_stsd_codec(f, start: int, end: int) -> Optional[str]:
    for pos, size, typ, header_size in _iter_boxes(f, start, end):
        if typ != "stsd":
            continue
        try:
            f.seek(pos + header_size)
        except Exception:
            return None
        header = _read_exact(f, 8)
        if not header:
            return None
        entry_count = _u32(header[4:8])
        if entry_count < 1:
            return None
        entry = _read_exact(f, 8)
        if not entry:
            return None
        fmt = entry[4:8].decode("latin-1", errors="replace")
        if fmt and fmt.strip("\x00"):
            return fmt.strip("\x00")
        return None
    return None


def _select_video_track(f, moov_start: int, moov_end: int) -> Optional[tuple[Optional[int], Optional[int], Optional[str]]]:
    for trak_pos, trak_size, trak_typ, trak_hdr in _iter_boxes(f, moov_start, moov_end):
        if trak_typ != "trak":
            continue
        trak_start = trak_pos + trak_hdr
        trak_end = trak_pos + trak_size

        handler = None
        for mdia_pos, mdia_size, mdia_typ, mdia_hdr in _iter_boxes(f, trak_start, trak_end):
            if mdia_typ != "mdia":
                continue
            mdia_start = mdia_pos + mdia_hdr
            mdia_end = mdia_pos + mdia_size
            handler = _parse_hdlr_handler(f, mdia_start, mdia_end)
            if handler:
                if handler != "vide":
                    handler = None
                break

        if handler != "vide":
            continue

        width, height = _parse_tkhd_dims(f, trak_start, trak_end)

        codec = None
        for mdia_pos, mdia_size, mdia_typ, mdia_hdr in _iter_boxes(f, trak_start, trak_end):
            if mdia_typ != "mdia":
                continue
            mdia_start = mdia_pos + mdia_hdr
            mdia_end = mdia_pos + mdia_size
            for minf_pos, minf_size, minf_typ, minf_hdr in _iter_boxes(f, mdia_start, mdia_end):
                if minf_typ != "minf":
                    continue
                minf_start = minf_pos + minf_hdr
                minf_end = minf_pos + minf_size
                for stbl_pos, stbl_size, stbl_typ, stbl_hdr in _iter_boxes(f, minf_start, minf_end):
                    if stbl_typ != "stbl":
                        continue
                    stbl_start = stbl_pos + stbl_hdr
                    stbl_end = stbl_pos + stbl_size
                    codec = _parse_stsd_codec(f, stbl_start, stbl_end)
                    break
                break
            break

        return width, height, codec

    return None


def extract_video_metadata(path: Path) -> VideoExtractResult:
    try:
        size = path.stat().st_size
    except Exception:
        return VideoExtractResult(ok=False, data={}, error_code="read_error", message_short="stat 실패")

    try:
        with path.open("rb") as f:
            moov = _find_moov(f, int(size))
            if not moov:
                return VideoExtractResult(ok=False, data={}, error_code="no_moov", message_short="moov 박스 없음")

            moov_pos, moov_size, moov_hdr = moov
            moov_start = moov_pos + moov_hdr
            moov_end = moov_pos + moov_size

            duration_sec, created_iso = _parse_mvhd(f, moov_start, moov_end)
            track = _select_video_track(f, moov_start, moov_end)

            video: dict[str, Any] = {"container": "mp4"}
            if isinstance(duration_sec, (int, float)):
                video["duration_sec"] = float(duration_sec)
            if created_iso:
                video["container_created"] = created_iso

            if track:
                width, height, codec = track
                if isinstance(width, int) and width > 0:
                    video["width"] = width
                if isinstance(height, int) and height > 0:
                    video["height"] = height
                if isinstance(codec, str) and codec.strip():
                    video["codec"] = codec.strip()

            if len(video.keys()) <= 1:
                return VideoExtractResult(ok=False, data={}, error_code="no_video_meta", message_short="비디오 메타 없음")

            return VideoExtractResult(ok=True, data={"video": video})

    except OSError:
        return VideoExtractResult(ok=False, data={}, error_code="read_error", message_short="읽기 실패")
    except Exception as e:
        return VideoExtractResult(ok=False, data={}, error_code="extract_error", message_short=_short_message(e))
