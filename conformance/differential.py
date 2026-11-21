"""Development-only differential checks; external tools never enter the runtime."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def execute(command: list[str]) -> str:
    result = subprocess.run(command, capture_output=True, text=True, encoding="utf-8",
                            errors="replace", timeout=90)
    if len(result.stdout) > 16 * 1024 * 1024 or len(result.stderr) > 1024 * 1024:
        raise ValueError("differential tool output exceeds the verification limit")
    if result.returncode:
        raise ValueError(f"{Path(command[0]).name} failed ({result.returncode}): {result.stderr[-1200:]}")
    return result.stdout


def text(value: dict) -> str | None:
    if "Text" in value:
        t = value["Text"]
        return t["text"].strip(" \0") if t.get("lossless") else None
    if "Float" in value:
        number = struct.unpack(">d", int(value["Float"]).to_bytes(8, "big"))[0]
        return format(number, ".15g") if math.isfinite(number) else None
    for kind in ("Unsigned", "Signed"):
        if kind in value:
            return str(value[kind])
    return None


def observations(payload: dict) -> dict[str, set[str]]:
    if payload.get("ok") is not True:
        raise ValueError("MetaXtract did not produce a successful command envelope")
    records = payload["result"]["records"]
    if len(records) != 1:
        raise ValueError("inspect did not return exactly one source record")
    fields: dict[str, set[str]] = {}
    for observation in records[0]["observations"]:
        value = text(observation.get("interpreted_value") or observation["raw_value"])
        if value is not None:
            fields.setdefault(observation["tag"]["name"], set()).add(value)
    return fields


def compare(fields: dict[str, set[str]], key: str, expected: object) -> dict:
    normalized = str(expected).strip(" \0")
    return {"field": key, "reference": normalized, "actual": sorted(fields.get(key, set())),
            "matches": fields.get(key, set()) == {normalized}}


def validate(cli: Path, exiftool: Path, mediainfo: Path | None) -> dict:
    versions = {"exiftool": execute([str(exiftool), "-ver"]).strip()}
    if mediainfo is not None:
        versions["mediainfo"] = execute([str(mediainfo), "--Version"]).strip()
    results = []
    cameras = json.loads((ROOT / "conformance/corpus/native-camera-captures.json").read_text())["cases"]
    media = json.loads((ROOT / "conformance/corpus/native-media-library.json").read_text())["cases"]
    with tempfile.TemporaryDirectory(prefix="metaxtract differential 한글 ") as temporary:
        directory = Path(temporary)
        for index, case in enumerate(cameras):
            data = base64.b64decode(case["base64"], validate=True)
            path = directory / f"camera-{index}.jpg"
            path.write_bytes(data)
            reference = json.loads(execute([str(exiftool), "-json", "-n", "-Make", "-Model",
                                           "-DateTimeOriginal", str(path)]))[0]
            actual = observations(json.loads(execute([str(cli), "inspect", str(path)])))
            checks = []
            for key, field in (("Make", "make"), ("Model", "model"), ("DateTimeOriginal", "datetime_original")):
                if key in reference:
                    checks.append(compare(actual, field, reference[key]))
            if not checks or not {"Make", "Model"}.issubset(reference):
                raise ValueError("independent camera reference lacks required metadata")
            if path.read_bytes() != data:
                raise ValueError("differential verification modified source bytes")
            results.append({"case": case["name"], "sha256": hashlib.sha256(data).hexdigest(),
                            "reference_tool": "exiftool", "checks": checks})
        if mediainfo is not None:
            for index, case in enumerate(media):
                if case["parser"] != "media.inspect" or not {"sample_rate", "channels"}.issubset(case["fields"]):
                    continue
                data = base64.b64decode(case["base64"], validate=True)
                path = directory / f"media-{index}.bin"
                path.write_bytes(data)
                reference = json.loads(execute([str(mediainfo), "--Output=JSON", str(path)]))
                tracks = [track for track in reference["media"]["track"] if track["@type"] == "Audio"]
                if len(tracks) != 1:
                    raise ValueError("independent media reference lacks one audio stream")
                actual = observations(json.loads(execute([str(cli), "inspect", str(path)])))
                checks = [compare(actual, key, tracks[0][external]) for key, external in
                          (("sample_rate", "SamplingRate"), ("channels", "Channels"))]
                if path.read_bytes() != data:
                    raise ValueError("media verification modified source bytes")
                results.append({"case": case["name"], "sha256": hashlib.sha256(data).hexdigest(),
                                "reference_tool": "mediainfo", "checks": checks})
            if not any(r["reference_tool"] == "mediainfo" for r in results):
                raise ValueError("no media cases were independently checked")
    mismatches = sum(not c["matches"] for r in results for c in r["checks"])
    return {"schema": 1, "ok": mismatches == 0, "tool_versions": versions,
            "scope": "Make/Model/capture time; audio sampling rate/channels; padding and scalar representation normalized",
            "mismatches": mismatches, "cases": results}


def tool(value: str | None, default: str) -> Path:
    path = value or shutil.which(default)
    if path is None:
        raise ValueError(default + " is required for this development check")
    return Path(path).resolve(strict=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--metaxtract", type=Path)
    source.add_argument("--dist", type=Path, help="check the CLI from a verified distribution archive")
    parser.add_argument("--exiftool")
    parser.add_argument("--mediainfo", help="also require independent audio stream comparisons")
    args = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix="metaxtract differential runtime ") as temporary:
            provenance = None
            if args.dist:
                sys.path.insert(0, str(ROOT / "packaging"))
                from verify import check_inventory, check_sources, find_package, unpack
                archive = find_package(args.dist)
                root = unpack(archive, Path(temporary))
                manifest = check_inventory(root)
                check_sources(archive, manifest)
                cli = root / manifest["executables"]["cli"]
                provenance = manifest["git_commit"]
            else:
                cli = args.metaxtract.resolve(strict=True)
            result = validate(cli, tool(args.exiftool, "exiftool"),
                              tool(args.mediainfo, "mediainfo") if args.mediainfo else None)
            result["git_commit"] = provenance
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(json.dumps({"ok": False, "error": str(error)}))
        return 1
    print(json.dumps(result, ensure_ascii=False))
    return int(not result["ok"])


if __name__ == "__main__":
    raise SystemExit(main())
