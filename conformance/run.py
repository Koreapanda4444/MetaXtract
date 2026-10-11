from __future__ import annotations

import argparse
import base64
import json
import struct
import subprocess
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
    from metaxtract.core.models import normalize_relative_path

    seen = set()
    for case in corpus.get("cases", []):
        relative = normalize_relative_path(case["path"])
        if relative.casefold() in seen:
            raise ValueError(f"duplicate corpus path: {relative}")
        seen.add(relative.casefold())
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


def _compare_native(corpus: dict[str, Any], expected: dict[str, Any], release: bool) -> tuple[list[str], int]:
    requests = ["INFO", "SCHEMA"]
    wanted = ["INFO\t1\t1\t1", "SCHEMA\tOK"]
    png = b"\x89PNG\r\n\x1a\n" + struct.pack(">I4sIIBBBBBI", 13, b"IHDR", 1, 1, 8, 2, 0, 0, 0, 0)
    transport = bytearray(377)
    transport[0] = transport[188] = transport[376] = 0x47
    detection_cases = [
        (b"", ""),
        (b"plain text", ""),
        (png, "png:HeaderValidated:0"),
        (png[:8], "png:Truncated:0"),
        (png[:16] + b"\0\0\0\0" + png[20:], "png:Invalid:0"),
        (b"\xff\xd8\xff", "jpeg:Signature:0"),
        (b"II*\0\0\0\0\0", "tiff:HeaderValidated:0"),
        (b"MM\0+\0\4\0\0" + bytes(8), "bigtiff:Invalid:0"),
        (b"RIFF" + struct.pack("<I", 4) + b"WEBP", "webp:HeaderValidated:0"),
        (b"RIFF" + struct.pack("<I", 400) + b"WAVE", "wav:Truncated:0"),
        (b"GIF89a" + struct.pack("<HHBBB", 1, 1, 0, 0, 0), "gif:HeaderValidated:0"),
        (b"BM" + struct.pack("<III", 26, 0, 26) + struct.pack("<IHHHH", 12, 1, 1, 1, 24), "bmp:HeaderValidated:0"),
        (b"\0\0\1\0\1\0", "ico:HeaderValidated:0"),
        (b"fLaC\x80\0\0\x22", "flac:HeaderValidated:0"),
        (b"OggS" + bytes(23), "ogg:HeaderValidated:0"),
        (b"ID3\4\0\0" + bytes(4), "mp3:HeaderValidated:0"),
        (b"ID3\4\0\0\x80\0\0\0", "mp3:Invalid:0"),
        (struct.pack(">I4s4sI4s", 20, b"ftyp", b"avif", 0, b"mif1"), "avif:HeaderValidated:0"),
        (struct.pack(">I4s4sI", 16, b"ftyp", b"isom", 0), "mp4:HeaderValidated:0"),
        (b"prefix%PDF-1.7", "pdf:HeaderValidated:6"),
        (b"%PDF-a.7", "pdf:Invalid:0"),
        (b"PK\3\4", "zip:Signature:0"),
        (b"PK\3\4xx%PDF-1.7", "pdf:HeaderValidated:6,zip:Signature:0"),
        (b"MAC ", "ape:Signature:0"),
        (b"wvpk" + struct.pack("<I", 24) + bytes(24), "wavpack:HeaderValidated:0"),
        (b"\x1a\x45\xdf\xa3", "ebml:Signature:0"),
        (bytes(transport), "mpeg_ts:HeaderValidated:0"),
    ]
    for data, result in detection_cases:
        requests.append(f"DETECT\t{data.hex()}\t{len(data)}\t65536\t10")
        wanted.append("DETECT\t" + result)
    for source_max, read_max, observation_max, failure in [
        (3, 65536, 10, "SourceTooLarge"),
        (len(png), 1, 10, "ReadBudgetExceeded"),
        (len(png), 65536, 0, "ObservationLimitExceeded"),
    ]:
        requests.append(f"DETECT\t{png.hex()}\t{source_max}\t{read_max}\t{observation_max}")
        wanted.append(f"ERR\t{failure}\ttrue")
    for case in expected["path_cases"]:
        requests.append("PATH\t" + case["input"].encode().hex())
        wanted.append("PATH\tERR" if case.get("error") else "PATH\tOK\t" + case["normalized"].encode().hex())

    def read_case(kind: str, value: str, data: bytes, maximum: int, single: int, total: int, reads: list[tuple[int, int]]) -> None:
        requests.append("\t".join([kind, value, str(maximum), str(single), str(total), ",".join(f"{offset}:{length}" for offset, length in reads)]))
        if single == 0 or total == 0:
            wanted.append("ERR\tInvalidRequest\tfalse")
            return
        if len(data) > maximum:
            wanted.append("ERR\tSourceTooLarge\ttrue")
            return
        consumed = 0
        for offset, length in reads:
            if length > single:
                wanted.append("ERR\tInvalidRequest\ttrue")
            elif offset > len(data):
                wanted.append("ERR\tOutOfRange\ttrue")
            else:
                payload = data[offset:offset + length]
                if consumed + len(payload) > total:
                    wanted.append("ERR\tReadBudgetExceeded\ttrue")
                else:
                    consumed += len(payload)
                    wanted.append(f"CHECK\t{len(data)}\t{len(payload)}" if kind == "CHECK" else f"READ\t{len(data)}\t{payload.hex()}")

    with tempfile.TemporaryDirectory(prefix="metaxtract-native-conformance-") as temporary:
        root = Path(temporary)
        _materialize(root, corpus)
        for case in corpus["cases"]:
            data = base64.b64decode(case["base64"], validate=True)
            reads = [(0, 0), (0, 7), (len(data) - 1, 8), (len(data), 8), (len(data) + 1, 0), (2**64 - 1, 1), (0, 33)]
            read_case("MEM", data.hex(), data, len(data), 32, 64, reads)
            read_case("FILE", str(root / case["path"]).encode().hex(), data, len(data), 32, 64, reads)
            read_case("CHECK", str(root / case["path"]).encode().hex(), data, len(data), 32, 64, [(0, 7), (0, 33)])

        data = bytes(range(256)) * 256 + b"end"
        file = root / "large-input.bin"
        file.write_bytes(data)
        value = str(file).encode().hex()
        read_case("FILE", value, data, len(data), 32, 64, [(65530, 32), (0, 32), (0, 32), (len(data), 0)])
        read_case("FILE", value, data, len(data) - 1, 32, 64, [(0, 1)])
        read_case("MEM", "", b"", 0, 1, 1, [(0, 1), (1, 0)])
        read_case("MEM", "61626364", b"abcd", 4, 4, 4, [(0, 4), (0, 1), (4, 0)])
        read_case("FILE", value, data, len(data), 0, 64, [(0, 1)])

        for maximum, wall_time, result in [(len(data), 30000, "UnsupportedParser\ttrue"), (len(data) - 1, 30000, "SourceTooLarge\ttrue"), (len(data), 0, "InvalidRequest\tfalse")]:
            requests.append(f"RUN\t{value}\t{maximum}\t{wall_time}")
            wanted.append("ERR\t" + result)
        shrinking = root / "shrinking.bin"
        shrinking.write_bytes(b"short")
        requests.append("SHRINK\t" + str(shrinking).encode().hex())
        wanted.append("ERR\tReadFailure\ttrue")

        shrinking_detection = root / "shrinking-detection.bin"
        shrinking_detection.write_bytes(b"PNG")
        requests.append("SHRINK_DETECT\t" + str(shrinking_detection).encode().hex())
        wanted.append("ERR\tReadFailure\ttrue")

        requests.append("PARSE\t000a141e\t4\t8\t4\t30000\t2")
        wanted.extend(["PARSE\t4\t8", "PARSE\t4\t8"])
        for opcode, code in [(1, "ReadBudgetExceeded"), (2, "AllocationBudgetExceeded"), (3, "DecodeBudgetExceeded"), (4, "ContainerDepthExceeded"), (5, "EntryLimitExceeded"), (6, "ObservationLimitExceeded"), (7, "Internal"), (8, "DeadlineExceeded")]:
            wall_time = 1 if opcode == 8 else 30000
            requests.append(f"PARSE\t{opcode:02x}0a141e\t4\t8\t4\t{wall_time}\t1")
            wanted.append(f"ERR\t{code}\t{'false' if opcode == 7 else 'true'}")

        command = ["cargo", "run", "--locked", "--quiet", "-p", "metaxtract-native", "--features", "conformance", "--example", "conformance"]
        if release:
            command.append("--release")
        try:
            result = subprocess.run(command, cwd=ROOT, input="\n".join(requests) + "\n", text=True, capture_output=True, timeout=300)
        except (OSError, subprocess.TimeoutExpired) as exc:
            return [f"native conformance could not run: {exc}"], 0
        if result.returncode:
            detail = result.stderr.strip()
            if len(detail) > 6000:
                detail = detail[:4000] + "\n... compiler output omitted ...\n" + detail[-2000:]
            return [f"native conformance failed ({result.returncode}): {detail}"], 0
        actual = result.stdout.splitlines()
        if actual != wanted:
            mismatches = [f"response {index}: expected {want!r}, got {got!r}" for index, (want, got) in enumerate(zip(wanted, actual), 1) if want != got]
            if len(actual) != len(wanted):
                mismatches.append(f"response count: expected {len(wanted)}, got {len(actual)}")
            return mismatches, len(wanted)
        return [], len(wanted)


def run(*, python_only: bool = False, release: bool = False) -> dict[str, Any]:
    sys.path.insert(0, str(PYTHON_SOURCE))
    corpus = _read_json(ROOT / "conformance" / "corpus" / "files.json")
    expected = _read_json(ROOT / "conformance" / "expected" / "python.json")

    failures = []
    failures.extend(_compare_records(corpus, expected))
    failures.extend(_compare_paths(expected))
    failures.extend(_compare_frozen_baseline(expected))
    native_cases = 0
    if not python_only:
        native_failures, native_cases = _compare_native(corpus, expected, release)
        failures.extend(native_failures)
    return {
        "ok": not failures,
        "cases": len(corpus.get("cases", [])),
        "path_cases": len(expected.get("path_cases", [])),
        "native_contract_cases": native_cases,
        "native_enabled": not python_only,
        "native_record_parity": "pending format adapters",
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run MetaXtract migration conformance checks")
    parser.add_argument("--json", action="store_true", help="emit a compact JSON result")
    parser.add_argument("--python-only", action="store_true", help="check only the frozen Python baseline")
    parser.add_argument("--release", action="store_true", help="check the release Rust/C++ build")
    args = parser.parse_args()

    result = run(python_only=args.python_only, release=args.release)
    if args.json:
        print(json.dumps(result, separators=(",", ":"), sort_keys=True))
    elif result["ok"]:
        print(
            "Conformance passed: "
            f"{result['cases']} Python files, {result['path_cases']} paths, "
            f"{result['native_contract_cases']} Rust/C++ contract checks"
        )
    else:
        print("Conformance failed", file=sys.stderr)
        for failure in result["failures"]:
            print(f"- {failure}", file=sys.stderr)
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
