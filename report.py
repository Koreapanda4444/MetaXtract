
from __future__ import annotations
import report_html

import csv
import io
import json
from copy import deepcopy
from typing import Any

import index_store
import rules_privacy
import rules_timeline


def _as_text(v: Any) -> str:
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    return str(v)


def _flatten(obj: Any) -> dict[str, Any]:
    out: dict[str, Any] = {}

    def walk(v: Any, prefix: str) -> None:
        if isinstance(v, dict):
            for k in sorted(v.keys()):
                key = str(k)
                nxt = f"{prefix}.{key}" if prefix else key
                walk(v.get(k), nxt)
            return
        if isinstance(v, list):
            out[prefix] = json.dumps(v, ensure_ascii=False, sort_keys=True)
            return
        out[prefix] = v

    if isinstance(obj, dict):
        walk(obj, "")
    else:
        out[""] = obj

    return out


def _sorted_records(records: list[dict]) -> list[dict]:
    def keyfn(r: dict) -> str:
        f = r.get("file")
        if isinstance(f, dict):
            p = f.get("path")
            if isinstance(p, str):
                return p
        raw_path = r.get("path")
        return str(raw_path or "")

    return sorted(records, key=keyfn)


def _apply_rules(records: list[dict], *, redact: bool) -> list[dict]:
    out: list[dict] = []
    for r in records:
        rc = deepcopy(r)
        rules_privacy.apply_privacy_intelligence(rc, redact=bool(redact))
        rules_timeline.apply_timeline_checks(rc)
        out.append(rc)
    return out


def _pick_template_item(record: dict, template: str) -> dict:
    file_obj = record.get("file") if isinstance(record.get("file"), dict) else {}

    if template == "privacy":
        sig = record.get("signals") if isinstance(record.get("signals"), dict) else {}
        return {
            "file": {
                "path": file_obj.get("path"),
                "name": file_obj.get("name"),
                "ext": file_obj.get("ext"),
                "size_bytes": file_obj.get("size_bytes"),
            },
            "identity": {
                "author": (record.get("identity") or {}).get("author") if isinstance(record.get("identity"), dict) else None,
            },
            "capture": {
                "make": (record.get("capture") or {}).get("make") if isinstance(record.get("capture"), dict) else None,
                "model": (record.get("capture") or {}).get("model") if isinstance(record.get("capture"), dict) else None,
                "software": (record.get("capture") or {}).get("software") if isinstance(record.get("capture"), dict) else None,
                "datetime_original": (record.get("capture") or {}).get("datetime_original") if isinstance(record.get("capture"), dict) else None,
            },
            "geo": record.get("geo") if isinstance(record.get("geo"), dict) else {},
            "signals": {
                "privacy_flags": sig.get("privacy_flags", {}),
                "risk_summary": sig.get("risk_summary", ""),
                "risk_score": sig.get("risk_score", 0),
                "reason_codes": sig.get("reason_codes", []),
            },
        }

    if template == "forensics":
        return {
            "file": {
                "path": file_obj.get("path"),
                "name": file_obj.get("name"),
                "ext": file_obj.get("ext"),
                "size_bytes": file_obj.get("size_bytes"),
            },
            "os_times": record.get("os_times") if isinstance(record.get("os_times"), dict) else {},
            "meta_times": record.get("meta_times") if isinstance(record.get("meta_times"), dict) else {},
            "signals": {
                "timeline_flags": (record.get("signals") or {}).get("timeline_flags") if isinstance(record.get("signals"), dict) else {},
                "privacy_flags": (record.get("signals") or {}).get("privacy_flags") if isinstance(record.get("signals"), dict) else {},
            },
            "raw": {
                "extract_error": (record.get("raw") or {}).get("extract_error") if isinstance(record.get("raw"), dict) else None,
            },
        }

    if template == "content":
        capture = record.get("capture") if isinstance(record.get("capture"), dict) else {}
        identity = record.get("identity") if isinstance(record.get("identity"), dict) else {}
        media = record.get("media") if isinstance(record.get("media"), dict) else {}
        sig = record.get("signals") if isinstance(record.get("signals"), dict) else {}

        name = _as_text(file_obj.get("name"))
        ext = _as_text(file_obj.get("ext"))
        author = _as_text(identity.get("author"))
        software = _as_text(capture.get("software"))
        model = _as_text(capture.get("model"))
        dur = media.get("duration_sec")
        width = media.get("width")
        height = media.get("height")

        lines: list[str] = []
        if name:
            lines.append(f"제목: {name}")
        if author:
            lines.append(f"작성자: {author}")
        if model:
            lines.append(f"기기: {model}")
        if software:
            lines.append(f"도구: {software}")
        if isinstance(dur, (int, float)):
            lines.append(f"길이: {float(dur):.2f}s")
        if isinstance(width, int) and isinstance(height, int) and width > 0 and height > 0:
            lines.append(f"해상도: {width}x{height}")
        summary = _as_text(sig.get("risk_summary"))
        if summary:
            first = summary.splitlines()[0].strip()
            if first:
                lines.append(f"주의: {first}")
        if not lines:
            lines.append(f"파일: {name or ''}{ext}")

        return {
            "file": {
                "path": file_obj.get("path"),
                "name": file_obj.get("name"),
                "ext": file_obj.get("ext"),
            },
            "card": "\n".join(lines),
        }

    return {
        "file": {
            "path": file_obj.get("path"),
            "name": file_obj.get("name"),
            "ext": file_obj.get("ext"),
            "size_bytes": file_obj.get("size_bytes"),
        }
    }


def _render_txt(items: list[dict], template: str) -> str:
    blocks: list[str] = []

    for item in items:
        f = item.get("file") if isinstance(item.get("file"), dict) else {}
        path = _as_text(f.get("path"))
        name = _as_text(f.get("name"))

        if template == "privacy":
            sig = item.get("signals") if isinstance(item.get("signals"), dict) else {}
            flags = sig.get("privacy_flags") if isinstance(sig.get("privacy_flags"), dict) else {}
            summary = _as_text(sig.get("risk_summary"))
            risk_score = _as_text(sig.get("risk_score"))
            reason_codes = sig.get("reason_codes", [])
            flags_line = " ".join(
                [
                    f"has_gps={_as_text(flags.get('has_gps'))}",
                    f"has_author={_as_text(flags.get('has_author'))}",
                    f"has_device_model={_as_text(flags.get('has_device_model'))}",
                    f"has_software_trace={_as_text(flags.get('has_software_trace'))}",
                    f"has_precise_time={_as_text(flags.get('has_precise_time'))}",
                ]
            )
            block = "\n".join([
                path or name,
                summary.strip(),
                f"risk_score: {risk_score}",
                f"reason_codes: {','.join(reason_codes)}",
                flags_line
            ]).strip()
            blocks.append(block)
            continue

        if template == "forensics":
            os_times = item.get("os_times") if isinstance(item.get("os_times"), dict) else {}
            meta_times = item.get("meta_times") if isinstance(item.get("meta_times"), dict) else {}
            sig = item.get("signals") if isinstance(item.get("signals"), dict) else {}
            tl = sig.get("timeline_flags") if isinstance(sig.get("timeline_flags"), dict) else {}
            reasons = tl.get("reason_codes")
            if isinstance(reasons, list):
                reasons_text = ";".join([_as_text(x) for x in reasons if _as_text(x)])
            else:
                reasons_text = _as_text(reasons)

            lines = [
                path or name,
                f"timeline: {_as_text(tl.get('short_explain'))}",
                f"mismatch: {_as_text(tl.get('time_mismatch'))}",
                f"reasons: {reasons_text}",
                f"os.mtime: {_as_text(os_times.get('mtime'))}",
                f"os.ctime: {_as_text(os_times.get('ctime'))}",
                f"meta.created: {_as_text(meta_times.get('created'))}",
                f"meta.modified: {_as_text(meta_times.get('modified'))}",
                f"meta.digitized: {_as_text(meta_times.get('digitized'))}",
            ]
            blocks.append("\n".join(lines).strip())
            continue

        if template == "content":
            card = _as_text(item.get("card"))
            head = path or name
            blocks.append("\n".join([head, card]).strip())
            continue

        blocks.append((path or name).strip())

    return "\n\n".join([b for b in blocks if b]) + ("\n" if blocks else "")


def _render_json(session: dict | None, items: list[dict]) -> str:
    payload = {
        "session": session or {},
        "items": items,
    }
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, indent=2) + "\n"


def _render_csv(items: list[dict]) -> str:
    flat_rows = [_flatten(it) for it in items]
    cols: list[str] = sorted({k for row in flat_rows for k in row.keys() if k})
    if not cols:
        cols = ["file.path"]

    buf = io.StringIO(newline="")
    w = csv.DictWriter(buf, fieldnames=cols, extrasaction="ignore", lineterminator="\n")
    w.writeheader()
    for row in flat_rows:
        out_row = {k: _as_text(row.get(k)) for k in cols}
        w.writerow(out_row)
    return buf.getvalue()


def generate(index_path: str, *, fmt: str, template: str, redact: bool = False) -> str:
    session, records = index_store.split_session_and_records(index_path)

    return generate_from_records(session, records, fmt=fmt, template=template, redact=redact)


def generate_from_records(
    session: dict | None,
    records: list[dict],
    *,
    fmt: str,
    template: str,
    redact: bool = False,
) -> str:
    prepared = _apply_rules(records, redact=bool(redact))
    prepared = _sorted_records(prepared)

    items = [_pick_template_item(r, template) for r in prepared]

    if fmt == "json":
        return _render_json(session, items)
    if fmt == "csv":
        return _render_csv(items)
    if fmt == "txt":
        return _render_txt(items, template)
    if fmt == "html":
        return report_html.generate_html_report(session, items)

    raise ValueError(f"unsupported format: {fmt}")
