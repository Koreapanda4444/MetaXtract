from typing import Any, Dict


def build_manifest(records, options) -> Dict[str, Any]:
    return {
        "case_id": options.get("case_id"),
        "created_at": options.get("created_at", "1980-01-01T00:00:00Z"),
        "notes": options.get("notes", ""),
        "record_count": len(records),
        "hashes": options.get("hashes", []),
        "redacted": options.get("redacted", False),
        "includes_files": bool(options.get("includes_files", False)),
        "original_files": list(options.get("original_files", [])),
        "artifacts": list(options.get("artifacts", [])),
    }
