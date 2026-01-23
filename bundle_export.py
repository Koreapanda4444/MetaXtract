
import os
import zipfile
import shutil
import tempfile
import json
from typing import List, Optional
import report
import report_html

def export_case(
    index_path: str,
    out_zip: str,
    include_original: bool = False,
    extra_formats: Optional[List[str]] = None,
    sanitize_logs_dir: Optional[str] = None,
):
    """
    index_path: 스캔 결과(jsonl)
    out_zip: 내보낼 zip 경로
    include_original: 원본 파일 포함 여부
    extra_formats: 추가 리포트 포맷 (예: ["json", "txt", "html"])
    sanitize_logs_dir: 로그 폴더 경로 (예: __metaxtract_logs/sanitize)
    """
    if extra_formats is None:
        extra_formats = ["json", "txt", "html"]
    with tempfile.TemporaryDirectory() as tmpdir:
        # 1. index.jsonl 복사
        index_dst = os.path.join(tmpdir, "index.jsonl")
        shutil.copy2(index_path, index_dst)
        # 2. 리포트 생성
        session, records = report.index_store.split_session_and_records(index_path)
        for fmt in extra_formats:
            if fmt == "html":
                html = report_html.generate_html_report(session, records)
                with open(os.path.join(tmpdir, f"report.{fmt}"), "w", encoding="utf-8") as f:
                    f.write(html)
            else:
                out = report.generate_from_records(session, records, fmt=fmt, template="privacy", redact=False)
                with open(os.path.join(tmpdir, f"report.{fmt}"), "w", encoding="utf-8") as f:
                    f.write(out)
        # 3. hashes.txt (모든 파일 sha256)
        import hashlib
        def sha256sum(path):
            h = hashlib.sha256()
            with open(path, "rb") as f:
                for chunk in iter(lambda: f.read(8192), b""):
                    h.update(chunk)
            return h.hexdigest()
        hashes = {}
        for fname in os.listdir(tmpdir):
            fpath = os.path.join(tmpdir, fname)
            if os.path.isfile(fpath):
                hashes[fname] = sha256sum(fpath)
        with open(os.path.join(tmpdir, "hashes.txt"), "w", encoding="utf-8") as f:
            for k, v in hashes.items():
                f.write(f"{k}\t{v}\n")
        # 4. meta_manifest.json
        manifest = {
            "index": "index.jsonl",
            "reports": [f"report.{fmt}" for fmt in extra_formats],
            "hashes": "hashes.txt",
        }
        if sanitize_logs_dir and os.path.isdir(sanitize_logs_dir):
            logs_dst = os.path.join(tmpdir, "sanitize_logs")
            shutil.copytree(sanitize_logs_dir, logs_dst)
            manifest["sanitize_logs"] = "sanitize_logs/"
        with open(os.path.join(tmpdir, "meta_manifest.json"), "w", encoding="utf-8") as f:
            json.dump(manifest, f, ensure_ascii=False, indent=2)
        # 5. 원본 파일 포함 옵션
        if include_original:
            with open(index_path, encoding="utf-8") as f:
                for line in f:
                    if line.strip().startswith("{"):
                        try:
                            rec = json.loads(line)
                        except Exception:
                            continue
                        file = rec.get("file")
                        if file and "path" in file and os.path.isfile(file["path"]):
                            dst = os.path.join(tmpdir, os.path.basename(file["path"]))
                            if not os.path.exists(dst):
                                shutil.copy2(file["path"], dst)
        # 6. zip으로 묶기
        with zipfile.ZipFile(out_zip, "w", zipfile.ZIP_DEFLATED) as z:
            for root, _, files in os.walk(tmpdir):
                for fname in files:
                    fpath = os.path.join(root, fname)
                    arcname = os.path.relpath(fpath, tmpdir)
                    z.write(fpath, arcname)
