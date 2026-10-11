from __future__ import annotations

import json
import shutil
import subprocess
from collections import Counter
from typing import Any, Dict, List, Tuple

from .normalize import as_float, as_int, fraction_to_float, json_safe, normalize_metadata
from ..core.files import PathLike


def _ffprobe_available() -> bool:
    return shutil.which("ffprobe") is not None


def _stream_summary(stream: Dict[str, Any]) -> Dict[str, Any]:
    summary: Dict[str, Any] = {}
    for field in (
        "index",
        "codec_type",
        "codec_name",
        "codec_long_name",
        "profile",
        "pix_fmt",
        "channel_layout",
    ):
        value = stream.get(field)
        if value not in (None, ""):
            summary[field] = json_safe(value)

    for field in ("width", "height", "channels", "sample_rate", "bit_rate"):
        value = as_int(stream.get(field))
        if value is not None:
            summary[field] = value

    duration = as_float(stream.get("duration"))
    if duration is not None:
        summary["duration"] = duration

    frame_rate_raw = stream.get("avg_frame_rate") or stream.get("r_frame_rate")
    frame_rate = fraction_to_float(frame_rate_raw)
    if frame_rate_raw not in (None, ""):
        summary["frame_rate_raw"] = str(frame_rate_raw)
    if frame_rate is not None:
        summary["frame_rate"] = frame_rate

    tags = stream.get("tags")
    if isinstance(tags, dict) and tags:
        summary["tags"] = json_safe(tags)
    return summary


def extract_video(path: PathLike) -> Tuple[Dict[str, Any], List[str]]:
    metadata: Dict[str, Any] = {}
    warnings: List[str] = []

    if not _ffprobe_available():
        warnings.append("ffprobe_not_found")
        return metadata, warnings

    command = [
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
        process = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        warnings.append("ffprobe_timeout")
        return metadata, warnings
    except OSError:
        warnings.append("ffprobe_failed_to_run")
        return metadata, warnings

    if process.returncode != 0:
        warnings.append("ffprobe_nonzero_exit")
        return metadata, warnings

    try:
        data = json.loads(process.stdout)
    except (TypeError, json.JSONDecodeError):
        warnings.append("ffprobe_invalid_json")
        return metadata, warnings
    if not isinstance(data, dict):
        warnings.append("ffprobe_invalid_json")
        return metadata, warnings

    format_data = data.get("format")
    if not isinstance(format_data, dict):
        format_data = {}
    metadata["video_duration"] = as_float(format_data.get("duration")) or 0.0
    metadata["video_format_name"] = str(format_data.get("format_name") or "")
    format_bit_rate = as_int(format_data.get("bit_rate"))
    if format_bit_rate is not None:
        metadata["video_bit_rate"] = format_bit_rate
    format_tags = format_data.get("tags")
    if isinstance(format_tags, dict) and format_tags:
        metadata["video_tags"] = json_safe(format_tags)

    raw_streams = data.get("streams")
    streams = [item for item in raw_streams or [] if isinstance(item, dict)]
    summaries = [_stream_summary(stream) for stream in streams]
    stream_types = Counter(str(stream.get("codec_type") or "unknown") for stream in streams)
    metadata["video_stream_count"] = len(streams)
    metadata["video_stream_types"] = dict(sorted(stream_types.items()))
    metadata["video_streams"] = summaries

    video_stream = next(
        (stream for stream in streams if stream.get("codec_type") == "video"),
        None,
    )
    if video_stream:
        metadata["video_codec"] = str(video_stream.get("codec_name") or "")
        metadata["video_width"] = as_int(video_stream.get("width")) or 0
        metadata["video_height"] = as_int(video_stream.get("height")) or 0
        metadata["video_pixel_format"] = str(video_stream.get("pix_fmt") or "")
        frame_rate_raw = video_stream.get("avg_frame_rate") or video_stream.get("r_frame_rate")
        frame_rate = fraction_to_float(frame_rate_raw)
        if frame_rate is not None:
            metadata["video_frame_rate"] = frame_rate

    audio_stream = next(
        (stream for stream in streams if stream.get("codec_type") == "audio"),
        None,
    )
    if audio_stream:
        metadata["audio_codec"] = str(audio_stream.get("codec_name") or "")
        sample_rate = as_int(audio_stream.get("sample_rate"))
        channels = as_int(audio_stream.get("channels"))
        if sample_rate is not None:
            metadata["audio_sample_rate"] = sample_rate
        if channels is not None:
            metadata["audio_channels"] = channels
        metadata["audio_channel_layout"] = str(audio_stream.get("channel_layout") or "")

    return normalize_metadata(metadata), warnings
