from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, Literal, Optional

import index_store
import schema


DigestAlgo = Literal["sha256"]


@dataclass(frozen=True)
class VerifyIssue:
    line: int
    code: str
    message: str


@dataclass(frozen=True)
class VerifyResult:
    ok: bool
    record_count: int
    issues: list[VerifyIssue]
    session: Optional[dict[str, Any]] = None
    index_digest: Optional[dict[str, str]] = None


def _is_nonempty_str(v: Any) -> bool:
    return isinstance(v, str) and bool(v.strip())


def _is_number(v: Any) -> bool:
    return isinstance(v, (int, float))


def _validate_session_header(session: dict[str, Any], *, line: int) -> list[VerifyIssue]:
    issues: list[VerifyIssue] = []

    if session.get("type") != "session":
        issues.append(VerifyIssue(line, "session.type", "첫 레코드는 type=session 이어야 합니다."))

    tool = session.get("tool")
    if not isinstance(tool, dict) or not _is_nonempty_str(tool.get("name")) or not _is_nonempty_str(tool.get("version")):
        issues.append(VerifyIssue(line, "session.tool", "tool.name/tool.version 형식이 올바르지 않습니다."))

    if not _is_nonempty_str(session.get("timestamp")):
        issues.append(VerifyIssue(line, "session.timestamp", "timestamp(ISO8601 문자열)가 필요합니다."))

    platform_obj = session.get("platform")
    if not isinstance(platform_obj, dict):
        issues.append(VerifyIssue(line, "session.platform", "platform 객체가 필요합니다."))

    scan = session.get("scan")
    if not isinstance(scan, dict):
        issues.append(VerifyIssue(line, "session.scan", "scan 객체가 필요합니다."))
    else:
        if not _is_nonempty_str(scan.get("root")):
            issues.append(VerifyIssue(line, "session.scan.root", "scan.root가 필요합니다."))
        if not isinstance(scan.get("recursive"), bool):
            issues.append(VerifyIssue(line, "session.scan.recursive", "scan.recursive는 bool 이어야 합니다."))
        exclude = scan.get("exclude")
        if exclude is not None and not isinstance(exclude, list):
            issues.append(VerifyIssue(line, "session.scan.exclude", "scan.exclude는 list 이어야 합니다."))
        if not _is_nonempty_str(scan.get("hash")):
            issues.append(VerifyIssue(line, "session.scan.hash", "scan.hash(none/sha256/md5)가 필요합니다."))

    return issues


def _validate_record_schema(rec: dict[str, Any], *, line: int) -> list[VerifyIssue]:
    issues: list[VerifyIssue] = []

    if not schema.has_top_level_keys(rec):
        missing = sorted(set(schema.TOP_LEVEL_KEYS) - set(rec.keys()))
        extra = sorted(set(rec.keys()) - set(schema.TOP_LEVEL_KEYS))
        msg = "최상위 키셋이 스키마(v1)와 일치하지 않습니다."
        if missing:
            msg += f" missing={missing}"
        if extra:
            msg += f" extra={extra}"
        issues.append(VerifyIssue(line, "record.schema", msg))
        return issues

    file_obj = rec.get("file")
    if not isinstance(file_obj, dict):
        issues.append(VerifyIssue(line, "record.file", "file 객체가 필요합니다."))
    else:
        if not _is_nonempty_str(file_obj.get("path")):
            issues.append(VerifyIssue(line, "record.file.path", "file.path가 필요합니다."))
        if not _is_nonempty_str(file_obj.get("name")):
            issues.append(VerifyIssue(line, "record.file.name", "file.name이 필요합니다."))
        if not isinstance(file_obj.get("ext"), str):
            issues.append(VerifyIssue(line, "record.file.ext", "file.ext는 문자열이어야 합니다."))
        if not isinstance(file_obj.get("size_bytes"), int):
            issues.append(VerifyIssue(line, "record.file.size_bytes", "file.size_bytes는 int 이어야 합니다."))

    os_times = rec.get("os_times")
    if not isinstance(os_times, dict):
        issues.append(VerifyIssue(line, "record.os_times", "os_times 객체가 필요합니다."))
    else:
        for k in ("atime", "mtime", "ctime"):
            if k not in os_times or not _is_number(os_times.get(k)):
                issues.append(VerifyIssue(line, f"record.os_times.{k}", f"os_times.{k}는 숫자여야 합니다."))

    signals = rec.get("signals")
    if not isinstance(signals, dict):
        issues.append(VerifyIssue(line, "record.signals", "signals 객체가 필요합니다."))
    else:
        pf = signals.get("privacy_flags")
        if not isinstance(pf, dict):
            issues.append(VerifyIssue(line, "record.signals.privacy_flags", "signals.privacy_flags는 dict 이어야 합니다."))

    return issues


def verify_index(
    index_path: str,
    *,
    summary: bool = False,
    summary_algo: DigestAlgo = "sha256",
) -> VerifyResult:
    issues: list[VerifyIssue] = []
    record_count = 0

    session: Optional[dict[str, Any]] = None
    expected_hash_algo: Optional[str] = None

    for line_no, line in index_store.iter_jsonl_lines(index_path):
        try:
            obj = json.loads(line)
        except Exception as e:
            issues.append(VerifyIssue(line_no, "json.invalid", f"JSON 파싱 실패: {e}"))
            continue

        if session is None:
            if not isinstance(obj, dict):
                issues.append(VerifyIssue(line_no, "session.missing", "세션 헤더가 필요합니다(첫 레코드)."))
                # 이후는 레코드로 계속 검사
            else:
                session = obj
                issues.extend(_validate_session_header(session, line=line_no))
                scan = session.get("scan")
                if isinstance(scan, dict):
                    h = scan.get("hash")
                    if isinstance(h, str) and h.strip() and h.strip().lower() != "none":
                        expected_hash_algo = h.strip().lower()
                continue

        # session 이후
        if isinstance(obj, dict) and obj.get("type") == "session":
            issues.append(VerifyIssue(line_no, "session.duplicate", "세션 헤더가 중복되었습니다."))
            continue

        if not isinstance(obj, dict):
            issues.append(VerifyIssue(line_no, "record.type", "레코드는 dict(JSON object) 이어야 합니다."))
            continue

        record_count += 1
        issues.extend(_validate_record_schema(obj, line=line_no))

        if expected_hash_algo is not None:
            hashes = obj.get("hashes")
            if not isinstance(hashes, dict) or not _is_nonempty_str(hashes.get(expected_hash_algo)):
                issues.append(
                    VerifyIssue(
                        line_no,
                        "record.hashes.missing",
                        f"해시 옵션({expected_hash_algo})이 켜져 있으나 record.hashes.{expected_hash_algo}가 없습니다.",
                    )
                )

    if session is None:
        issues.append(VerifyIssue(0, "session.missing", "세션 헤더(type=session)가 존재하지 않습니다."))

    digest: Optional[dict[str, str]] = None
    if summary:
        digest = {"algo": summary_algo, "hex": index_store.compute_file_digest(index_path, algo=summary_algo)}

    ok = len(issues) == 0
    return VerifyResult(ok=ok, record_count=record_count, issues=issues, session=session, index_digest=digest)


def format_summary_json(result: VerifyResult) -> str:
    payload: dict[str, Any] = {
        "ok": bool(result.ok),
        "records": int(result.record_count),
        "issues": [
            {"line": i.line, "code": i.code, "message": i.message}
            for i in result.issues
        ],
    }
    if result.index_digest is not None:
        payload["index_digest"] = result.index_digest
    return json.dumps(payload, ensure_ascii=False, indent=2)
