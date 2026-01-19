from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Literal, Optional

import index_store
import utils


DiffFormat = Literal["txt", "json"]
KeyField = Literal["path", "sha256"]


class DiffResult:
    def __init__(self) -> None:
        self.added: list[dict] = []
        self.removed: list[dict] = []
        self.changed: list[dict] = []


def _get_key(record: dict, key_field: KeyField) -> Optional[str]:
    """레코드에서 키 필드 값을 추출합니다."""
    if key_field == "path":
        return record.get("file", {}).get("path")
    elif key_field == "sha256":
        return record.get("hashes", {}).get("sha256")
    return None


def _extract_privacy_flags(record: dict) -> dict[str, bool]:
    """레코드에서 privacy flags를 추출합니다."""
    return record.get("signals", {}).get("privacy_flags", {})


def _detect_privacy_changes(before_flags: dict[str, bool], after_flags: dict[str, bool]) -> list[str]:
    """Privacy flags의 변화를 감지하고 카테고리 메시지를 생성합니다."""
    changes = []
    
    # GPS 제거/추가
    if before_flags.get("has_gps", False) and not after_flags.get("has_gps", False):
        changes.append("GPS removed")
    elif not before_flags.get("has_gps", False) and after_flags.get("has_gps", False):
        changes.append("GPS added")
    
    # Author 변경
    if before_flags.get("has_author", False) != after_flags.get("has_author", False):
        if after_flags.get("has_author", False):
            changes.append("Author added")
        else:
            changes.append("Author removed")
    
    # Device model 변경
    if before_flags.get("has_device_model", False) != after_flags.get("has_device_model", False):
        if after_flags.get("has_device_model", False):
            changes.append("Device model added")
        else:
            changes.append("Device model removed")
    
    # Software trace 변경
    if before_flags.get("has_software_trace", False) != after_flags.get("has_software_trace", False):
        if after_flags.get("has_software_trace", False):
            changes.append("Software trace added")
        else:
            changes.append("Software trace removed")
    
    # Precise time 변경
    if before_flags.get("has_precise_time", False) != after_flags.get("has_precise_time", False):
        if after_flags.get("has_precise_time", False):
            changes.append("Precise time added")
        else:
            changes.append("Precise time removed")
    
    return changes


def _detect_metadata_changes(before: dict, after: dict) -> dict[str, Any]:
    """메타데이터 변화를 감지합니다."""
    changes: dict[str, Any] = {}
    
    # Geo 변화
    before_geo = before.get("geo", {})
    after_geo = after.get("geo", {})
    if before_geo != after_geo:
        geo_changes = {}
        for key in ["lat", "lon", "alt_m", "precision_flag"]:
            if before_geo.get(key) != after_geo.get(key):
                geo_changes[key] = {
                    "before": before_geo.get(key),
                    "after": after_geo.get(key),
                }
        if geo_changes:
            changes["geo"] = geo_changes
    
    # Identity 변화
    before_identity = before.get("identity", {})
    after_identity = after.get("identity", {})
    if before_identity != after_identity:
        identity_changes = {}
        for key in ["author", "title", "last_modified_by"]:
            if before_identity.get(key) != after_identity.get(key):
                identity_changes[key] = {
                    "before": before_identity.get(key),
                    "after": after_identity.get(key),
                }
        if identity_changes:
            changes["identity"] = identity_changes
    
    # Capture 변화
    before_capture = before.get("capture", {})
    after_capture = after.get("capture", {})
    if before_capture != after_capture:
        capture_changes = {}
        for key in ["make", "model", "software", "datetime_original"]:
            if before_capture.get(key) != after_capture.get(key):
                capture_changes[key] = {
                    "before": before_capture.get(key),
                    "after": after_capture.get(key),
                }
        if capture_changes:
            changes["capture"] = capture_changes
    
    # Meta times 변화
    before_meta_times = before.get("meta_times", {})
    after_meta_times = after.get("meta_times", {})
    if before_meta_times != after_meta_times:
        meta_times_changes = {}
        for key in ["created", "modified", "digitized"]:
            if before_meta_times.get(key) != after_meta_times.get(key):
                meta_times_changes[key] = {
                    "before": before_meta_times.get(key),
                    "after": after_meta_times.get(key),
                }
        if meta_times_changes:
            changes["meta_times"] = meta_times_changes
    
    return changes


def _build_change_record(before: dict, after: dict, key_field: KeyField) -> dict:
    """변경 레코드를 구축합니다."""
    key_value = _get_key(before, key_field) or _get_key(after, key_field)
    
    before_flags = _extract_privacy_flags(before)
    after_flags = _extract_privacy_flags(after)
    privacy_changes = _detect_privacy_changes(before_flags, after_flags)
    
    metadata_changes = _detect_metadata_changes(before, after)
    
    change_record = {
        key_field: key_value,
        "privacy_changes": privacy_changes,
        "metadata_changes": metadata_changes,
    }
    
    return change_record


def diff(before_path: str, after_path: str, key_field: KeyField = "path") -> DiffResult:
    """두 JSONL 인덱스 파일을 비교하고 변화를 반환합니다."""
    # 세션 헤더와 레코드 분리
    _, before_records = index_store.split_session_and_records(before_path)
    _, after_records = index_store.split_session_and_records(after_path)
    
    # 키를 기준으로 매핑 생성
    before_map = {}
    for rec in before_records:
        key = _get_key(rec, key_field)
        if key:
            before_map[key] = rec
    
    after_map = {}
    for rec in after_records:
        key = _get_key(rec, key_field)
        if key:
            after_map[key] = rec
    
    result = DiffResult()
    
    # 추가된 항목
    for key in after_map:
        if key not in before_map:
            result.added.append(after_map[key])
    
    # 제거된 항목
    for key in before_map:
        if key not in after_map:
            result.removed.append(before_map[key])
    
    # 변경된 항목
    for key in before_map:
        if key in after_map:
            before_rec = before_map[key]
            after_rec = after_map[key]
            
            # 레코드가 다른지 확인 (file, os_times, hashes 제외)
            before_compare = {k: v for k, v in before_rec.items() 
                            if k not in ["file", "os_times", "hashes"]}
            after_compare = {k: v for k, v in after_rec.items() 
                           if k not in ["file", "os_times", "hashes"]}
            
            if before_compare != after_compare:
                change_record = _build_change_record(before_rec, after_rec, key_field)
                result.changed.append(change_record)
    
    return result


def format_as_txt(result: DiffResult, key_field: KeyField) -> str:
    """DiffResult를 텍스트 형식으로 포맷합니다."""
    lines = []
    
    lines.append("=== Diff Report ===\n")
    
    # 요약
    lines.append(f"Added: {len(result.added)}")
    lines.append(f"Removed: {len(result.removed)}")
    lines.append(f"Changed: {len(result.changed)}")
    lines.append("")
    
    # 추가된 항목
    if result.added:
        lines.append("--- Added ---")
        for rec in result.added:
            key_value = _get_key(rec, key_field)
            lines.append(f"  + {key_value}")
        lines.append("")
    
    # 제거된 항목
    if result.removed:
        lines.append("--- Removed ---")
        for rec in result.removed:
            key_value = _get_key(rec, key_field)
            lines.append(f"  - {key_value}")
        lines.append("")
    
    # 변경된 항목
    if result.changed:
        lines.append("--- Changed ---")
        for change in result.changed:
            key_value = change.get(key_field)
            lines.append(f"  ~ {key_value}")
            
            # Privacy 변화
            privacy_changes = change.get("privacy_changes", [])
            if privacy_changes:
                lines.append(f"    Privacy: {', '.join(privacy_changes)}")
            
            # Metadata 변화
            metadata_changes = change.get("metadata_changes", {})
            if metadata_changes:
                for category, changes in metadata_changes.items():
                    for field, diff in changes.items():
                        lines.append(f"    {category}.{field}: {diff['before']} → {diff['after']}")
            
            lines.append("")
    
    return "\n".join(lines)


def format_as_json(result: DiffResult, key_field: KeyField) -> str:
    """DiffResult를 JSON 형식으로 포맷합니다."""
    output = {
        "summary": {
            "added": len(result.added),
            "removed": len(result.removed),
            "changed": len(result.changed),
        },
        "added": [_get_key(rec, key_field) for rec in result.added],
        "removed": [_get_key(rec, key_field) for rec in result.removed],
        "changed": result.changed,
    }
    return json.dumps(output, ensure_ascii=False, indent=2)


def generate(before_path: str, after_path: str, *, 
             key_field: KeyField = "path",
             fmt: DiffFormat = "txt") -> str:
    """Diff 리포트를 생성합니다."""
    result = diff(before_path, after_path, key_field=key_field)
    
    if fmt == "json":
        return format_as_json(result, key_field)
    else:
        return format_as_txt(result, key_field)
