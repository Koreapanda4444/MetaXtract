from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import random
import struct
import subprocess
import sys
import tempfile
import sqlite3
import zipfile
from contextlib import closing
from dataclasses import asdict
from datetime import datetime
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


def _fuzz_requests(count: int, seed: int) -> list[tuple[str, str]]:
    """Replay retained malformed seeds and deterministic mutations through the FFI."""
    seeds = []
    paths = [ROOT / "conformance/corpus/images.json", *sorted((ROOT / "conformance/corpus").glob("native-*.json")), ROOT / "conformance/corpus/fuzz-seeds.json"]
    for path in paths:
        for case in _read_json(path)["cases"]:
            data = base64.b64decode(case["base64"], validate=True)[:65536]
            seeds.append((path.stem + "/" + case.get("name", case.get("id", "seed")), case["parser"], data, case.get("budget", "normal")))
    rng = random.Random(seed)
    modes = ("normal", "read", "allocation", "decoded", "depth", "entries", "observations", "short", "member", "ratio", "allocation-near")
    result = []
    # Retained regressions always replay, even when new mutation count is zero.
    for label, parser, data, mode in seeds:
        if label.startswith("fuzz-seeds/"):
            result.append((label, f"FUZZ\t{label}\t{data.hex()}\t{parser}\t{mode}"))
    for index in range(count):
        label, parser, original, _ = seeds[rng.randrange(len(seeds))]
        data = bytearray(original)
        operation = index % 4
        if operation == 0:
            del data[rng.randrange(len(data) + 1):]
        elif operation == 1 and data:
            at = rng.randrange(len(data))
            data[at] ^= 1 << rng.randrange(8)
        elif operation == 2:
            at = rng.randrange(len(data) + 1)
            data[at:at + 8] = bytes([0xff if (index // 4) % 2 else 0]) * 8
        else:
            data.extend(rng.randbytes(min(32, 65536 - len(data))))
        label = f"seed{seed:x}-{index}-{label}"
        mode = modes[(index // 4) % len(modes)]
        result.append((label, f"FUZZ\t{label}\t{data[:65536].hex()}\t{parser}\t{mode}"))
    return result


def _compare_native(corpus: dict[str, Any], expected: dict[str, Any], release: bool,
                    fuzz_cases: int = 256, fuzz_seed: int = 0x4d58) -> tuple[list[str], int]:
    requests = ["INFO", "SCHEMA"]
    wanted = ["INFO\t2\t1\t22", "SCHEMA\tOK"]
    request_labels = ["build info", "schema invariants"]
    extraction_cases = _read_json(ROOT / "conformance" / "corpus" / "images.json")["cases"]
    for corpus_path in sorted((ROOT / "conformance" / "corpus").glob("native-*.json")):
        extraction_cases.extend(_read_json(corpus_path)["cases"])
    for case in extraction_cases:
        request_labels.append("extract " + case.get("name", case["parser"]))
        data = base64.b64decode(case["base64"], validate=True)
        selected = ",".join(case["fields"])
        request = f"EXTRACT\t{data.hex()}\t{case['parser']}\t{selected}\t{case['read_max']}\t{case['observation_max']}"
        request += f"\t{case.get('allocation_max', 512 * 1024 * 1024)}\t{case.get('decoded_max', 256 * 1024 * 1024)}\t{case.get('depth_max', 32)}\t{case.get('entries_max', 100000)}\t{case.get('member_max', 64 * 1024 * 1024)}\t{case.get('ratio_max', 1000)}"
        requests.append(request)
        fields = ";".join(key + "=" + "|".join(values) for key, values in case["fields"].items())
        wanted.append(f"EXTRACT\t{case['status']}\t{fields}")
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

        request_labels.extend(["protocol command"] * (len(wanted) - len(request_labels)))
        for label, request in _fuzz_requests(fuzz_cases, fuzz_seed):
            requests.append(request)
            request_labels.append(label)
            wanted.append("FUZZ\tOK")

        command = ["cargo", "run", "--locked", "--quiet", "-p", "metaxtract-native", "--features", "conformance", "--example", "conformance"]
        if release:
            command.append("--release")
        try:
            # Compilation of pinned native libraries is separate from the bounded
            # protocol runtime. Keep Cargo's DLL search setup when running.
            build_command = command.copy()
            build_command[1] = "build"
            build = subprocess.run(build_command, cwd=ROOT, text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=1800)
            if build.returncode:
                detail = build.stderr.strip()
                if len(detail) > 6000:
                    detail = detail[:4000] + "\n... compiler output omitted ...\n" + detail[-2000:]
                return [f"native conformance build failed ({build.returncode}): {detail}"], 0
            result = subprocess.run(command, cwd=ROOT, input="\n".join(requests) + "\n", text=True, encoding="utf-8", errors="replace", capture_output=True, timeout=300)
        except (OSError, subprocess.TimeoutExpired) as exc:
            return [f"native conformance could not run: {exc}"], 0
        if result.returncode:
            detail = result.stderr.strip()
            if len(detail) > 6000:
                detail = detail[:4000] + "\n... compiler output omitted ...\n" + detail[-2000:]
            return [f"native conformance failed ({result.returncode}): {detail}"], 0
        actual = result.stdout.splitlines()
        if actual != wanted:
            mismatches = [f"response {index} ({request_labels[index - 1] if index <= len(request_labels) else 'protocol command'}): expected {want!r}, got {got!r}" for index, (want, got) in enumerate(zip(wanted, actual), 1) if want != got]
            if len(actual) != len(wanted):
                mismatches.append(f"response count: expected {len(wanted)}, got {len(actual)}")
            return mismatches, len(wanted)
        return [], len(wanted)


def _cli_command(release: bool, executable: Path | None = None) -> list[str]:
    if executable is not None:
        return [str(executable.resolve())]
    return ["cargo", "run", "--locked", "--quiet", "-p", "metaxtract-cli", *(["--release"] if release else []), "--"]


def _compare_cli(release: bool, executable: Path | None = None) -> list[str]:
    """Exercise installed command contracts, including checkpoint replay and non-clobber."""
    build = ["cargo", "build", "--locked", "--quiet", "-p", "metaxtract-cli", "-p", "metaxtract-worker"]
    command = _cli_command(release, executable)
    if release:
        build.append("--release")
    try:
        if executable is None:
            subprocess.run(build, cwd=ROOT, check=True, capture_output=True, timeout=1800)

        def invoke(*args: object, code: int = 0, stdin: str | None = None) -> dict[str, Any]:
            p = subprocess.run(command + [*map(str, args)], cwd=ROOT, text=True, encoding="utf-8",
                               capture_output=True, input=stdin, timeout=120)
            if p.returncode != code:
                raise ValueError(f"CLI {args[0]} exit {p.returncode}: {p.stderr[-2000:]} {p.stdout[:2000]}")
            envelope = json.loads(p.stdout if code in (0, 5, 6) else p.stderr.splitlines()[-1])
            if envelope["ok"] != (code in (0, 5, 6)):
                raise ValueError("CLI success/error envelope differs")
            return envelope.get("result", envelope)

        with tempfile.TemporaryDirectory(prefix="metaxtract-cli-contract-") as temporary:
            root = Path(temporary)
            source = root / "evidence"
            source.mkdir()
            (source / "one.txt").write_text("Evidence one / 근거 하나", encoding="utf-8")
            (source / "two.txt").write_text("Evidence two", encoding="utf-8")
            case = source / "case.mxcase"
            paused = invoke("scan", source, "--case", case, "--workers", 1, "--pause-after", 1, code=6)
            invoke("scan", source, "--case", case, "--resume", paused["run"], "--scan", paused["scan_id"])
            checked = invoke("verify", case)
            if checked["records"] != 2:
                raise ValueError("checkpoint replay duplicated or lost records")
            page = invoke("search", case, paused["scan_id"], "--limit", 1)
            if len(page["items"]) != 1 or page["next"] is None:
                raise ValueError("CLI pagination omitted its cursor")
            invoke("search", case, paused["scan_id"], "--limit", 1, "--cursor", json.dumps(page["next"]))
            invoke("search", case, paused["scan_id"], "--save", "contract")
            invoke("search", case, paused["scan_id"], "--save", "contract", "--order", "size")
            output = root / "records.jsonl"
            invoke("export", case, paused["scan_id"], output)
            invoke("export", case, paused["scan_id"], output, code=3)
            invoke("scan", source, "--case", case, "--typo", "x", code=2)
            invoke("verify", root / "absent.mxcase", code=3)
            # A real decoded frame catches preview policy/worker regressions that a
            # successful hex-only fallback would otherwise conceal.
            import zlib
            def png_chunk(kind: bytes, data: bytes) -> bytes:
                return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)
            image = root / "image.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
                              + png_chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00")) + png_chunk(b"IEND", b""))
            image_scan = invoke("scan", image, "--case", case)
            image_id = invoke("search", case, image_scan["scan_id"])["items"][0]["id"]
            preview = invoke("preview", case, image_id, image)
            if preview["rgb"] != [255, 0, 0] or not preview["verified"]:
                raise ValueError(f"native PNG preview unavailable: {preview['notices']}")
            invoke("preview", case, image_id, source / "one.txt", code=4)
            # Keep a signed report verifiable after reopening its detached receipt.
            key, public = root / "key.pem", root / "public.pem"
            report, receipt = root / "signed.html", root / "receipt.json"
            password = "contract-only-password\n"
            invoke("keygen", key, public, "--name", "contract", "--password-stdin", stdin=password)
            invoke("report", case, paused["scan_id"], report, "--key", key,
                   "--receipt", receipt, "--password-stdin", stdin=password)
            invoke("verify-report", report, receipt, public)
            report.write_bytes(report.read_bytes() + b"changed")
            invoke("verify-report", report, receipt, public, code=4)
            # Each stored ZIP expands by less than 2x; only the shared tree
            # ceiling can catch the cumulative cost across recursive calls.
            import io
            import zipfile
            nested = bytes(range(256)) * 16
            for _ in range(3):
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_STORED) as archive:
                    archive.writestr("inside.zip", nested)
                nested = buffer.getvalue()
            container = root / "nested.zip"
            container.write_bytes(nested)
            limited = invoke("inspect", container, "--max-expansion-ratio", 2)
            codes = {d["code"] for r in limited["records"] for d in r["diagnostics"]}
            if "recursive.ExpansionRatioExceeded" not in codes or len(limited["records"]) < 2:
                raise ValueError("recursive expansion budget reset between nested members")
            expanded = invoke("inspect", container, "--max-expansion-ratio", 0)
            if len(expanded["records"]) != 4:
                raise ValueError("bounded nested ZIP exploration lost members")
            for flag, code in [("--max-member-bytes", "MemberSizeExceeded"),
                               ("--max-tree-bytes", "DecodeBudgetExceeded")]:
                limited = invoke("inspect", container, flag, 32)
                codes = {d["code"] for r in limited["records"] for d in r["diagnostics"]}
                if len(limited["records"]) != 1 or "recursive." + code not in codes:
                    raise ValueError(f"{flag} did not stop decoding before queuing a member")
                if not limited["records"][0]["observations"]:
                    raise ValueError("resource limit discarded earlier archive metadata")
            invoke("inspect", container, "--max-member-bytes", 0, code=2)

        return []
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as exc:
        return [f"CLI workflow contract failed: {exc}"]


def _raw_value(value: dict[str, Any]) -> Any:
    kind, payload = next(iter(value.items()))
    if kind == "Text":
        return payload["text"]
    if kind in ("Unsigned", "Signed", "Bool"):
        return payload
    if kind == "Rational":
        return payload["numerator"] / payload["denominator"] if payload["denominator"] else None
    if kind == "Sequence":
        return [_raw_value(v) for v in payload]
    return None


def _python_projection(record: dict[str, Any]) -> dict[str, Any]:
    """Project native facts onto the captured Python fields, leaving richer fields intact."""
    def values(name: str, namespace: str = "") -> list[Any]:
        found = []
        for observation in record["observations"]:
            tag = observation["tag"]
            if tag.get("name", "") == name and tag["namespace"].startswith(namespace):
                value = _raw_value(observation.get("interpreted_value") or observation["raw_value"])
                if value not in found:
                    found.append(value)
        return found

    def one(name: str, namespace: str = "") -> Any:
        found = values(name, namespace)
        return found[0] if len(found) == 1 else None

    source = record["source"]
    mime = source.get("detected_media_type") or source.get("declared_media_type") or "application/octet-stream"
    metadata: dict[str, Any] = {}
    if mime in ("image/png", "image/jpeg"):
        namespace = "image.png" if mime == "image/png" else "image.jpeg"
        metadata.update(format="PNG" if mime == "image/png" else "JPEG",
                        width=one("width", namespace), height=one("height", namespace))
        if mime == "image/png":
            metadata["mode"] = {0: "L", 2: "RGB", 3: "P", 4: "LA", 6: "RGBA"}.get(one("color_type", namespace))
            if one("color_type", namespace) == 0 and one("bit_depth", namespace) == 16:
                metadata["mode"] = "I;16"
            animated = any(o["tag"].get("name") == "type" and _raw_value(o.get("interpreted_value") or o["raw_value"]) == "acTL" for o in record["observations"])
            frames = one("frame_count", namespace) if animated else 1
        else:
            metadata["mode"] = {1: "L", 3: "RGB", 4: "CMYK"}.get(one("components", namespace))
            frames = 1
        metadata.update(frames=frames, animated=frames is not None and frames > 1)
        for field in ("make", "model", "software", "artist", "lens_model", "datetime_original", "orientation"):
            found = []
            for observation in record["observations"]:
                if ".exif." in observation["tag"]["namespace"] and observation["tag"].get("name") == field:
                    value = _raw_value(observation.get("interpreted_value") or observation["raw_value"])
                    if value not in found:
                        found.append(value)
            if len(found) == 1:
                metadata["exif_" + field] = found[0]
        if "exif_orientation" in metadata:
            metadata["exif_orientation_label"] = {1: "normal", 2: "mirrored_horizontal", 3: "rotated_180", 4: "mirrored_vertical", 5: "mirrored_horizontal_rotated_270", 6: "rotated_90", 7: "mirrored_horizontal_rotated_90", 8: "rotated_270"}.get(metadata["exif_orientation"], "unknown")
        locations = [c["value"]["GeoPoint"] for c in record["claims"] if "GeoPoint" in c["value"]]
        if locations:
            if any(p != locations[0] for p in locations):
                raise ValueError("native GPS alternatives conflict in the reference fixture")
            point = locations[0]
            metadata.update(gps_latitude=point["latitude"], gps_longitude=point["longitude"])
            if point["altitude_meters"] is not None:
                metadata["gps_altitude_m"] = point["altitude_meters"]
    elif mime == "application/pdf":
        metadata["pdf_encrypted"] = bool(one("encrypted", "pdf.document"))
        pages = one("page_count", "pdf.document")
        if pages is not None:
            metadata["pages"] = pages
        for field in ("Title", "Author", "Subject", "Keywords", "Creator", "Producer"):
            value = one(field, "pdf.info")
            if value is None:
                value = one("/" + field, "pdf.info")
            if value is not None:
                metadata["pdf_" + field.lower()] = value
    elif mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document":
        for old, new in (("paragraphs", "paragraph_count"), ("tables", "table_count"), ("sections", "section_count")):
            metadata["docx_" + old] = one(new, "document.package")
        dc = "{http://purl.org/dc/elements/1.1/}"
        cp = "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}"
        terms = "{http://purl.org/dc/terms/}"
        properties = {"title": dc + "title", "author": dc + "creator", "last_modified_by": cp + "lastModifiedBy",
                      "subject": dc + "subject", "keywords": cp + "keywords", "category": cp + "category",
                      "comments": dc + "description", "language": dc + "language", "identifier": dc + "identifier",
                      "version": cp + "version", "content_status": cp + "contentStatus", "revision": cp + "revision",
                      "created": terms + "created", "modified": terms + "modified", "last_printed": cp + "lastPrinted"}
        for key, name in properties.items():
            value = one(name, "document.package")
            if key == "revision" and value is not None:
                value = int(value)
            if key in ("created", "modified", "last_printed") and value is not None:
                value = datetime.fromisoformat(value.replace("Z", "+00:00")).isoformat()
            metadata["docx_" + key] = value
    digest = next(d for d in record["digests"] if d["algorithm"] == "Sha256")
    return {"path": source["path"]["root"], "mime": mime, "size_bytes": source["identity"]["size_bytes"],
            "sha256": bytes(digest["bytes"]).hex(), "metadata": metadata}


def _compare_application_parity(release: bool, executable: Path | None = None) -> tuple[list[str], int]:
    """Exercise real mixed-engine records and captured legacy artifacts through the public CLI."""
    command = _cli_command(release, executable)
    expected = _read_json(ROOT / "conformance/expected/python.json")
    completed = 0

    def invoke(*args: object, code: int = 0) -> dict[str, Any]:
        p = subprocess.run(command + list(map(str, args)), cwd=ROOT, capture_output=True, text=True,
                           encoding="utf-8", errors="replace", timeout=180)
        if p.returncode != code:
            raise ValueError(f"acceptance {args[0]} exit {p.returncode}: {p.stderr[-1800:]} {p.stdout[:1000]}")
        envelope = json.loads(p.stdout or p.stderr.splitlines()[-1])
        return envelope.get("result", envelope)

    def sha(path: Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()

    try:
        with tempfile.TemporaryDirectory(prefix="metaxtract-acceptance-근거-") as temporary:
            root = Path(temporary)
            sources = root / "evidence"
            sources.mkdir()
            _materialize(sources, _read_json(ROOT / "conformance/corpus/files.json"))
            case = root / "acceptance.mxcase"
            scanned = invoke("scan", sources, "--case", case, "--workers", 1, "--decode-previews")
            scan = scanned["scan_id"]
            exported = root / "native.jsonl"
            invoke("export", case, scan, exported)
            records = [json.loads(row) for row in exported.read_text(encoding="utf-8").splitlines()]
            roots = {r["source"]["path"]["root"]: r for r in records if not r["source"]["path"]["nested"]}
            for wanted in expected["records"]:
                actual = _python_projection(roots[wanted["path"]])
                for field in ("path", "mime", "size_bytes", "sha256"):
                    if actual[field] != wanted[field]:
                        raise ValueError(f"record parity {wanted['path']} {field}: {actual[field]!r} != {wanted[field]!r}")
                for field, value in wanted["metadata"].items():
                    if actual["metadata"].get(field) != value:
                        raise ValueError(f"record parity {wanted['path']} {field}: {actual['metadata'].get(field)!r} != {value!r}")
                if "pdf_password_required" in wanted["warnings"] and not any(o["tag"].get("name") == "password_required" and _raw_value(o.get("interpreted_value") or o["raw_value"]) == 1 for o in roots[wanted["path"]]["observations"]):
                    raise ValueError("encrypted PDF lost its explicit password requirement")
                completed += 1
            if not any(c["key"] == "image.orientation" for c in roots["camera.jpg"]["claims"]):
                raise ValueError("image reconciliation is not wired into the application")
            for name, text in (("office.docx", "Retain this field"), ("rgba.png", "Retain unknown metadata")):
                if text not in json.dumps(roots[name], ensure_ascii=False):
                    raise ValueError("unknown metadata was lost: " + name)
            completed += 1
            originals = invoke("verify-originals", case, scan, sources)
            if not originals["verified"] or originals["verified_files"] != len(expected["records"]):
                raise ValueError("original verification did not cover the root files")
            physical_query = "path=evidence.txt"
            evidence = root / "selected.zip"
            invoke("export-evidence", case, scan, sources, evidence, "--query", physical_query)
            if invoke("verify-evidence", evidence)["records"] != 1:
                raise ValueError("selected evidence archive lost its metadata or original")
            second = root / "selected-again.zip"
            invoke("export-evidence", case, scan, sources, second, "--query", physical_query)
            if sha(evidence) != sha(second):
                raise ValueError("unchanged selected evidence did not export deterministically")
            tampered = root / "tampered.zip"
            with zipfile.ZipFile(evidence) as original, zipfile.ZipFile(tampered, "w") as changed:
                for name in original.namelist():
                    data = original.read(name)
                    changed.writestr(name, b"x" * len(data) if name.startswith("evidence/") else data)
            invoke("verify-evidence", tampered, code=4)
            unexpected = root / "unexpected.zip"
            unexpected.write_bytes(evidence.read_bytes())
            with zipfile.ZipFile(unexpected, "a") as changed:
                changed.writestr("../escape", b"no")
            invoke("verify-evidence", unexpected, code=4)
            (sources / "evidence.txt").write_bytes(b"modified")
            if invoke("verify-originals", case, scan, sources, code=4)["verified"]:
                raise ValueError("changed original passed verification")
            refused = root / "must-not-exist.zip"
            invoke("export-evidence", case, scan, sources, refused, "--query", physical_query, code=4)
            if refused.exists():
                raise ValueError("failed evidence export published a partial archive")
            completed += 1
            first, second = root / "snapshot.zip", root / "snapshot-again.zip"
            invoke("bundle", "export", case, first)
            invoke("bundle", "export", case, second)
            if sha(first) != sha(second):
                raise ValueError("unchanged case snapshot did not export deterministically")
            reopened = root / "reopened.mxcase"
            invoke("bundle", "open", first, reopened)
            if invoke("verify", reopened)["records"] != len(records):
                raise ValueError("portable snapshot lost typed records")
            completed += 1
            for view in ("timeline", "locations", "duplicates", "findings", "similar", "dashboard"):
                result = invoke("analyze", case, scan, "--view", view)
                if view == "locations" and not result["samples"]:
                    raise ValueError("native EXIF GPS did not reach geographic analysis")
            geographic = root / "locations.geojson"
            invoke("analyze", case, scan, "--view", "locations", "--format", "geojson", "--output", geographic)
            if not json.loads(geographic.read_text())["features"]:
                raise ValueError("geographic export lost native coordinates")
            completed += 1
            diagnostic = invoke("doctor")
            if not diagnostic["worker_available"] or any(p["id"].startswith("conformance") for p in diagnostic["parsers"]):
                raise ValueError("production diagnostics report a missing worker or fixture parser")
            cache = Path(str(case) + ".cache.sqlite")
            invoke("cache", "purge", cache)
            if invoke("verify", case)["records"] != len(records):
                raise ValueError("cache purge changed case records")
            invoke("cache", "purge", case, code=4)
            completed += 1
            legacy = expected["workflow_samples"]
            old_case, old_bundle, public = root / "python.mxc", root / "python.zip", root / "public.pem"
            old_case.write_bytes(base64.b64decode(legacy["case_base64"], validate=True))
            old_bundle.write_bytes(base64.b64decode(legacy["bundle_base64"], validate=True))
            public.write_text(legacy["trusted_public_pem"], encoding="utf-8")
            old_jsonl = root / "python.jsonl"
            old_jsonl.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in expected["records"]), encoding="utf-8")
            for kind, source in (("python", old_case), ("python-bundle", old_bundle), ("jsonl", old_jsonl)):
                before = sha(source)
                target = root / (kind + ".mxcase")
                invoke("case", "create", target)
                extra = ["--public-key", public] if kind == "python-bundle" else []
                result = invoke("import", target, source, "--kind", kind, *extra)
                if invoke("verify", target)["records"] != legacy["records"] or sha(source) != before:
                    raise ValueError("legacy import lost records or changed the source: " + kind)
                with closing(sqlite3.connect(target)) as db:
                    rows = [json.loads(row[0]) for row in db.execute("SELECT original FROM legacy_records")]
                    for row in rows:
                        if kind == "python":
                            sqlite_row = row.pop("original_sqlite_row")
                            if sqlite_row["path"] != row["path"] or json.loads(sqlite_row["metadata_json"]) != row["metadata"]:
                                raise ValueError("original Python SQLite row was changed")
                        row["metadata"].pop("mtime", None)
                        row["metadata"].pop("cache_hit", None)
                    actual_rows = sorted(rows, key=lambda row: row["path"])
                    if actual_rows != expected["records"]:
                        differences = [(a["path"], {k: [a.get(k), b.get(k)] for k in a.keys() | b.keys() if a.get(k) != b.get(k)})
                                       for a, b in zip(actual_rows, expected["records"]) if a != b]
                        raise ValueError("legacy fields differ: " + json.dumps(differences, ensure_ascii=False)[:1800])
                    if kind == "python":
                        if db.execute("SELECT count(*) FROM case_notes WHERE body=?", (legacy["notes"],)).fetchone()[0] != 1:
                            raise ValueError("Python case notes were lost")
                        if not db.execute("SELECT 1 FROM annotations WHERE note LIKE 'Preserved annotation%'").fetchone():
                            raise ValueError("Python annotation was lost")
                        saved = db.execute("SELECT query FROM saved_filters WHERE name='Reference images'").fetchone()
                        if saved is None or json.loads(saved[0])["Legacy"]["query"] != "mime:image/jpeg":
                            raise ValueError("legacy saved filter was changed")
                if not invoke("analyze", target, result["scans"][0], "--view", "locations")["samples"]:
                    raise ValueError("legacy GPS was not available for analysis")
                completed += 1
            invoke("import", root / "python-bundle.mxcase", old_bundle, "--kind", "python-bundle", code=4)
            completed += 1
            if os.name == "posix":
                unusual = root / "unusual"
                unusual.mkdir()
                # Linux permits arbitrary name bytes; other POSIX filesystems
                # exercise the same reversible codec with its reserved prefix.
                name = b"name-\xff" if sys.platform.startswith("linux") else b"~os-reserved"
                non_utf8 = os.fsencode(unusual) + b"/" + name
                with open(non_utf8, "wb") as output:
                    output.write(b"encoded evidence name")
                portable_case = root / "portable.mxcase"
                portable_scan = invoke("scan", unusual, "--case", portable_case)["scan_id"]
                if not invoke("verify-originals", portable_case, portable_scan, unusual)["verified"]:
                    raise ValueError("encoded OS evidence path did not reopen safely")
                os.unlink(non_utf8)
                outside = root / "outside"
                outside.write_bytes(b"encoded evidence name")
                os.symlink(os.fsencode(outside), non_utf8)
                if invoke("verify-originals", portable_case, portable_scan, unusual, code=4)["verified"]:
                    raise ValueError("evidence reopening followed a substituted symlink")
                completed += 1
        return [], completed
    except (OSError, ValueError, KeyError, StopIteration, subprocess.SubprocessError) as error:
        return [f"application parity gate failed: {error}"], completed


def run(*, python_only: bool = False, release: bool = False,
        fuzz_cases: int = 256, fuzz_seed: int = 0x4d58,
        cli_executable: Path | None = None) -> dict[str, Any]:
    sys.path.insert(0, str(PYTHON_SOURCE))
    corpus = _read_json(ROOT / "conformance" / "corpus" / "files.json")
    expected = _read_json(ROOT / "conformance" / "expected" / "python.json")

    failures = []
    failures.extend(_compare_records(corpus, expected))
    failures.extend(_compare_paths(expected))
    failures.extend(_compare_frozen_baseline(expected))
    native_cases = 0
    application_cases = 0
    if not python_only:
        native_failures, native_cases = _compare_native(corpus, expected, release, fuzz_cases, fuzz_seed)
        failures.extend(native_failures)
        failures.extend(_compare_cli(release, cli_executable))
        application_failures, application_cases = _compare_application_parity(release, cli_executable)
        failures.extend(application_failures)
    return {
        "ok": not failures,
        "cases": len(corpus.get("cases", [])),
        "path_cases": len(expected.get("path_cases", [])),
        "native_contract_cases": native_cases,
        "native_enabled": not python_only,
        "application_parity_checks": application_cases,
        "native_record_parity": "captured Python fields and migration workflows; bounded corpus, not exhaustive format equivalence",
        "failures": failures,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run MetaXtract migration conformance checks")
    parser.add_argument("--json", action="store_true", help="emit a compact JSON result")
    parser.add_argument("--python-only", action="store_true", help="check only the frozen Python baseline")
    parser.add_argument("--release", action="store_true", help="check the release Rust/C++ build")
    parser.add_argument("--cli-only", action="store_true", help="check application command contracts")
    parser.add_argument("--acceptance-only", action="store_true", help="check application contracts and captured Python parity")
    parser.add_argument("--cli-executable", type=Path, help="use a packaged CLI instead of cargo run")
    parser.add_argument("--fuzz-cases", type=int, default=256, help="deterministic mutation count (0..10000)")
    parser.add_argument("--fuzz-seed", type=lambda value: int(value, 0), default=0x4d58, help="reproducible mutation seed")
    args = parser.parse_args()
    if not 0 <= args.fuzz_cases <= 10000:
        parser.error("--fuzz-cases must be between 0 and 10000")

    if args.cli_only or args.acceptance_only:
        sys.path.insert(0, str(PYTHON_SOURCE))
        failures = _compare_cli(args.release, args.cli_executable)
        count = 0
        if args.acceptance_only:
            extra, count = _compare_application_parity(args.release, args.cli_executable)
            failures.extend(extra)
        print(json.dumps({"ok": not failures, "application_parity_checks": count, "failures": failures}, ensure_ascii=False))
        return int(bool(failures))
    result = run(python_only=args.python_only, release=args.release,
                 fuzz_cases=args.fuzz_cases, fuzz_seed=args.fuzz_seed,
                 cli_executable=args.cli_executable)
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
