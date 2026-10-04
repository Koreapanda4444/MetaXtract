from datetime import datetime, timezone
from typing import Any, Dict


def build_manifest(records, options) -> Dict[str, Any]:
    created_at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    return {
        "case_id": options.get("case_id"),
        "created_at": created_at,
        "notes": options.get("notes", ""),
        "record_count": len(records),
        "hashes": options.get("hashes", []),
        "redacted": options.get("redacted", False),
    }
