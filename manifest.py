from typing import Any, Dict
from datetime import datetime


def build_manifest(records, options) -> Dict[str, Any]:
    """
    케이스 번들용 manifest.json 생성
    Args:
        records: 스캔 결과 레코드 리스트
        options: 번들 생성 옵션 (case_id, notes 등)
    Returns:
        dict: manifest 데이터
    """
    manifest = {
        "case_id": options.get("case_id"),
        "created_at": datetime.utcnow().isoformat() + "Z",
        "notes": options.get("notes", ""),
        "record_count": len(records),
        "hashes": options.get("hashes", []),
        "redacted": options.get("redacted", False),
        # 기타 필요한 메타데이터 추가
    }
    return manifest
