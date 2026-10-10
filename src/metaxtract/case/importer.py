from __future__ import annotations

import json
import os
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from metaxtract.config import MAX_BUNDLE_CONTROL_BYTES, MAX_BUNDLE_MEMBERS, MAX_JSONL_BYTES
from metaxtract.core.jsonio import parse_jsonl_bytes, read_jsonl
from metaxtract.core.models import require_valid_records
from metaxtract.core.workspace import CaseWorkspace

from .signing import SIGNATURE_MEMBER
from .verify import verify_bundle


class ImportValidationError(ValueError):
    def __init__(self, issues: list[dict[str, str]]) -> None:
        self.issues = issues
        super().__init__(f"case bundle validation failed with {len(issues)} issue(s)")


@dataclass(frozen=True)
class ImportPayload:
    source_path: str
    source_kind: str
    origin: str
    records: list[dict[str, Any]]
    signed: bool = False
    manifest: dict[str, Any] | None = None


def bundle_is_signed(path: str | os.PathLike[str]) -> bool:
    bundle_path = Path(path).expanduser().absolute()
    with zipfile.ZipFile(bundle_path, "r") as archive:
        if len(archive.infolist()) > MAX_BUNDLE_MEMBERS:
            raise ValueError("case bundle contains too many entries")
        return SIGNATURE_MEMBER in archive.namelist()


def load_jsonl(path: str | os.PathLike[str]) -> ImportPayload:
    source = Path(path).expanduser().absolute()
    records = read_jsonl(source)
    return ImportPayload(
        source_path=str(source),
        source_kind="jsonl",
        origin="import_jsonl",
        records=records,
    )


def load_bundle(
    path: str | os.PathLike[str],
    *,
    public_key: str | os.PathLike[str] | None = None,
) -> ImportPayload:
    source = Path(path).expanduser().absolute()
    issues = verify_bundle(str(source), public_key=public_key)
    if issues:
        raise ImportValidationError(issues)

    with zipfile.ZipFile(source, "r") as archive:
        scan_info = archive.getinfo("scan.jsonl")
        if scan_info.file_size > MAX_JSONL_BYTES:
            raise ValueError(f"scan.jsonl exceeds {MAX_JSONL_BYTES} bytes")
        with archive.open(scan_info, "r") as stream:
            scan_data = stream.read(MAX_JSONL_BYTES + 1)
        if len(scan_data) > MAX_JSONL_BYTES:
            raise ValueError(f"scan.jsonl exceeds {MAX_JSONL_BYTES} bytes")
        records = require_valid_records(parse_jsonl_bytes(scan_data, source="scan.jsonl"))

        manifest_info = archive.getinfo("manifest.json")
        if manifest_info.file_size > MAX_BUNDLE_CONTROL_BYTES:
            raise ValueError(f"manifest.json exceeds {MAX_BUNDLE_CONTROL_BYTES} bytes")
        with archive.open(manifest_info, "r") as stream:
            manifest_data = stream.read(MAX_BUNDLE_CONTROL_BYTES + 1)
        if len(manifest_data) > MAX_BUNDLE_CONTROL_BYTES:
            raise ValueError(f"manifest.json exceeds {MAX_BUNDLE_CONTROL_BYTES} bytes")
        manifest = json.loads(manifest_data)
        if not isinstance(manifest, dict):
            raise ValueError("manifest.json must contain a JSON object")
        signed = SIGNATURE_MEMBER in archive.namelist()

    return ImportPayload(
        source_path=str(source),
        source_kind="bundle",
        origin="import_bundle",
        records=records,
        signed=signed,
        manifest=manifest,
    )


def import_payload(workspace: CaseWorkspace, payload: ImportPayload) -> str:
    return workspace.save_scan(
        payload.records,
        source_path=payload.source_path,
        source_kind=payload.source_kind,
        origin=payload.origin,
        imported_from=payload.source_path,
    )
