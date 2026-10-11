from __future__ import annotations

import argparse
import base64
import json
import sys
import tempfile
from dataclasses import asdict
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PYTHON_SOURCE = ROOT / "legacy" / "python" / "src"


def _read_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected a JSON object: {path}")
    return value


def _projection(record: Any) -> dict[str, Any]:
    row = asdict(record)
    metadata = dict(row.get("metadata") or {})
    metadata.pop("mtime", None)
    metadata.pop("cache_hit", None)
    row["metadata"] = metadata
    return row


def _materialize(root: Path, corpus: dict[str, Any]) -> None:
    for case in corpus.get("cases", []):
        relative = case["path"]
        payload = base64.b64decode(case["base64"], validate=True)
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(payload)


def _compare_records(corpus: dict[str, Any], expected: dict[str, Any]) -> list[str]:
    from metaxtract.core.scanner import scan_path

    with tempfile.TemporaryDirectory(prefix="metaxtract-conformance-") as temporary:
        root = Path(temporary)
        _materialize(root, corpus)
        records = [
            _projection(record)
            for record in scan_path(
                root,
                cache_enabled=False,
                max_files=max(len(corpus.get("cases", [])), 1),
            )
        ]

    actual = sorted(records, key=lambda row: row["path"])
    wanted = sorted(expected.get("records", []), key=lambda row: row["path"])
    if actual == wanted:
        return []
    return [
        "Python record projection differs from conformance/expected/python.json",
        json.dumps({"expected": wanted, "actual": actual}, indent=2, sort_keys=True),
    ]


def _compare_paths(expected: dict[str, Any]) -> list[str]:
    from metaxtract.core.models import normalize_relative_path

    failures = []
    for case in expected.get("path_cases", []):
        value = case["input"]
        if case.get("error"):
            try:
                normalize_relative_path(value)
            except ValueError:
                continue
            failures.append(f"unsafe path was accepted: {value!r}")
            continue

        try:
            actual = normalize_relative_path(value)
        except ValueError as exc:
            failures.append(f"valid path was rejected: {value!r}: {exc}")
            continue
        if actual != case["normalized"]:
            failures.append(
                f"path projection differs for {value!r}: {actual!r} != {case['normalized']!r}"
            )
    return failures


def _compare_frozen_baseline(expected: dict[str, Any]) -> list[str]:
    baseline = _read_json(ROOT / "reference" / "python" / "baseline.json")
    plain = next(
        (row for row in expected.get("records", []) if row.get("path") == "evidence.txt"),
        None,
    )
    frozen = baseline.get("cases", [{}])[0].get("scan_projection")
    if plain == frozen:
        return []
    return ["frozen Python baseline and conformance plain-text projection differ"]


def run() -> dict[str, Any]:
    sys.path.insert(0, str(PYTHON_SOURCE))
    corpus = _read_json(ROOT / "conformance" / "corpus" / "files.json")
    expected = _read_json(ROOT / "conformance" / "expected" / "python.json")

    failures = []
    failures.extend(_compare_records(corpus, expected))
    failures.extend(_compare_paths(expected))
    failures.extend(_compare_frozen_baseline(expected))
    return {
        "ok": not failures,
        "cases": len(corpus.get("cases", [])),
        "path_cases": len(expected.get("path_cases", [])),
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run MetaXtract migration conformance checks")
    parser.add_argument("--json", action="store_true", help="emit a compact JSON result")
    args = parser.parse_args()

    result = run()
    if args.json:
        print(json.dumps(result, separators=(",", ":"), sort_keys=True))
    elif result["ok"]:
        print(
            "Conformance passed: "
            f"{result['cases']} files and {result['path_cases']} path cases"
        )
    else:
        print("Conformance failed", file=sys.stderr)
        for failure in result["failures"]:
            print(f"- {failure}", file=sys.stderr)
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
