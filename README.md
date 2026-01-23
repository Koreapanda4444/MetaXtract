# MetaXtract

MetaXtract is a small, deterministic metadata scanner that can extract basic metadata from:

- Images (JPEG/PNG) via Pillow (EXIF + basic properties)
- PDFs via PyPDF2
- DOCX via python-docx
- Videos (optional) via ffprobe if available

It supports a CLI (`python cli.py scan <path>`) and a minimal GUI (`python gui.py`).

## Quick start

```bash
python cli.py scan tests/fixtures --out scan.jsonl
python cli.py report scan.jsonl --out report.json
python -m pytest
```
