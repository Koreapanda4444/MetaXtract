from __future__ import annotations

import json
import shutil
import subprocess
from typing import Any, Dict, List, Tuple

from utils import PathLike


def _ffprobe_available() -> bool:
    return shutil.which("ffprobe") is not None


def extract_video(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    md: Dict[str, Any] = {}
    warnings: List[str] = []

    if not _ffprobe_available():
        warnings.append("ffprobe_not_found")
        return md, warnings

    cmd = [
        "ffprobe",
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        str(path),
    ]

    try:
        proc = subprocess.run(cmd, check=False, capture_output=True, text=True)
    except Exception:
        warnings.append("ffprobe_failed_to_run")
        return md, warnings

    if proc.returncode != 0:
        warnings.append("ffprobe_nonzero_exit")
        return md, warnings

    try:
        data = json.loads(proc.stdout)
    except Exception:
        warnings.append("ffprobe_invalid_json")
        return md, warnings

    fmt = data.get("format") or {}
    md["video_duration"] = float(fmt.get("duration") or 0.0)
    md["video_format_name"] = str(fmt.get("format_name") or "")

    streams = data.get("streams") or []
    v_stream = next((s for s in streams if s.get("codec_type") == "video"), None)
    if v_stream:
        md["video_codec"] = str(v_stream.get("codec_name") or "")
        md["video_width"] = int(v_stream.get("width") or 0)
        md["video_height"] = int(v_stream.get("height") or 0)

    return md, warnings
