from __future__ import annotations

import argparse
from pathlib import Path
from typing import List, Optional

from bundle_export import export_bundle
from diff_report import diff_jsonl
from engine import scan_path
from report import build_report
from report_html import render_report_html
from utils import dumps_json, write_jsonl
from verify import verify_scan


def _cmd_scan(args: argparse.Namespace) -> int:
    records = scan_path(args.path)
    if args.out:
        write_jsonl(args.out, records)
    else:
        for r in records:
            print(dumps_json(r))
    return 0


def _cmd_report(args: argparse.Namespace) -> int:
    rep = build_report(args.scan)
    out_text = dumps_json(rep)
    if args.out:
        Path(args.out).write_text(out_text + "\n", encoding="utf-8")
    else:
        print(out_text)
    return 0


def _cmd_report_html(args: argparse.Namespace) -> int:
    html = render_report_html(args.scan)
    if args.out:
        Path(args.out).write_text(html + "\n", encoding="utf-8")
    else:
        print(html)
    return 0


def _cmd_diff(args: argparse.Namespace) -> int:
    summary = diff_jsonl(args.old, args.new)
    out_text = dumps_json(summary)
    if args.out:
        Path(args.out).write_text(out_text + "\n", encoding="utf-8")
    else:
        print(out_text)
    return 0


def _cmd_verify(args: argparse.Namespace) -> int:
    issues = verify_scan(args.scan, args.base)
    if issues:
        for i in issues:
            print(dumps_json(i))
        return 2
    print("OK")
    return 0


def _cmd_export_bundle(args: argparse.Namespace) -> int:
    export_bundle(args.scan, args.out)
    print(str(args.out))
    return 0


def _cmd_gui(_args: argparse.Namespace) -> int:
    from gui import main as gui_main

    gui_main()
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="metaxtract", description="MetaXtract metadata scanner")
    sub = p.add_subparsers(dest="cmd", required=True)

    scan = sub.add_parser("scan", help="scan a file or folder and emit JSONL")
    scan.add_argument("path", help="file or folder to scan")
    scan.add_argument("--out", help="output JSONL path (default: stdout)")
    scan.set_defaults(func=_cmd_scan)

    rep = sub.add_parser("report", help="build a JSON summary report from a scan.jsonl")
    rep.add_argument("scan", help="input scan.jsonl")
    rep.add_argument("--out", help="output report.json path (default: stdout)")
    rep.set_defaults(func=_cmd_report)

    rep_h = sub.add_parser("report-html", help="build an HTML report from a scan.jsonl")
    rep_h.add_argument("scan", help="input scan.jsonl")
    rep_h.add_argument("--out", help="output report.html path (default: stdout)")
    rep_h.set_defaults(func=_cmd_report_html)

    diff = sub.add_parser("diff", help="diff two scan.jsonl files")
    diff.add_argument("old", help="old scan.jsonl")
    diff.add_argument("new", help="new scan.jsonl")
    diff.add_argument("--out", help="output diff.json path (default: stdout)")
    diff.set_defaults(func=_cmd_diff)

    ver = sub.add_parser("verify", help="verify files on disk match hashes from scan.jsonl")
    ver.add_argument("scan", help="input scan.jsonl")
    ver.add_argument("base", help="base directory where files live")
    ver.set_defaults(func=_cmd_verify)

    exp = sub.add_parser("export-bundle", help="export scan + report into a ZIP bundle")
    exp.add_argument("scan", help="input scan.jsonl")
    exp.add_argument("out", help="output zip path")
    exp.set_defaults(func=_cmd_export_bundle)

    gui = sub.add_parser("gui", help="launch the GUI")
    gui.set_defaults(func=_cmd_gui)

    return p


def main(argv: Optional[List[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
