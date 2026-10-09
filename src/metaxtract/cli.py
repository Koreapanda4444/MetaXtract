from __future__ import annotations

import argparse
import sys
import zipfile
from pathlib import Path
from typing import Sequence

EXIT_SUCCESS = 0
EXIT_FAILURE = 1


def _cmd_doctor(_args: argparse.Namespace) -> int:
    from .doctor import print_doctor

    result = print_doctor()
    return EXIT_SUCCESS if result["ok"] else EXIT_FAILURE


def _cmd_scan(args: argparse.Namespace) -> int:
    from .core.cache import CacheStore
    from .core.jsonio import dumps_json, write_jsonl
    from .core.scanner import scan_path

    cache_enabled = args.cache != "off"
    cache_dir = args.cache_dir or ".metaxtract_cache"
    cache = CacheStore(cache_dir) if cache_enabled else None
    records = scan_path(
        args.path,
        cache=cache,
        cache_enabled=cache_enabled,
        max_files=args.max_files,
        include_hidden=args.include_hidden,
    )
    if args.out:
        write_jsonl(args.out, records)
    else:
        for record in records:
            print(dumps_json(record))
    return EXIT_SUCCESS


def _cmd_cache_purge(args: argparse.Namespace) -> int:
    from .core.cache import CacheStore

    cache_dir = args.cache_dir or ".metaxtract_cache"
    CacheStore(cache_dir).purge()
    print(f"Cache purged: {cache_dir}")
    return EXIT_SUCCESS


def _cmd_report(args: argparse.Namespace) -> int:
    from .core.jsonio import dumps_json, read_jsonl
    from .reporting.builder import build_report
    from .reporting.html import render_html

    records = read_jsonl(args.scan)
    fmt = getattr(args, "format", None) or ("html" if getattr(args, "html", False) else "json")
    if fmt == "html":
        output = render_html(records)
    else:
        output = dumps_json(build_report(args.scan))
    if args.out:
        Path(args.out).write_text(output + "\n", encoding="utf-8")
    else:
        print(output)
    return EXIT_SUCCESS


def _cmd_diff(args: argparse.Namespace) -> int:
    from .core.jsonio import dumps_json
    from .reporting.diff import diff_jsonl

    output = dumps_json(diff_jsonl(args.old, args.new))
    if args.out:
        Path(args.out).write_text(output + "\n", encoding="utf-8")
    else:
        print(output)
    return EXIT_SUCCESS


def _cmd_verify(args: argparse.Namespace) -> int:
    from .case.verify import verify_scan
    from .core.jsonio import dumps_json

    issues = verify_scan(args.scan, args.base)
    if issues:
        for issue in issues:
            print(dumps_json(issue))
        return EXIT_FAILURE
    print("OK")
    return EXIT_SUCCESS


def _cmd_verify_bundle(args: argparse.Namespace) -> int:
    from .case.verify import verify_bundle
    from .core.jsonio import dumps_json

    issues = verify_bundle(args.bundle, args.files_base, args.public_key)
    if issues:
        for issue in issues:
            print(dumps_json(issue))
        return EXIT_FAILURE
    print("OK")
    return EXIT_SUCCESS


def _cmd_export_case(args: argparse.Namespace) -> int:
    from .case.bundle import export_case_bundle

    export_case_bundle(
        args.scan,
        args.out,
        include_files=getattr(args, "include_files", False),
        redact=getattr(args, "redact", False),
        case_id=getattr(args, "case_id", None),
        notes=getattr(args, "notes", None),
        files_base=getattr(args, "files_base", None),
        signing_key=getattr(args, "signing_key", None),
    )
    return EXIT_SUCCESS


def _cmd_keygen(args: argparse.Namespace) -> int:
    from .case.signing import generate_signing_keypair

    generate_signing_keypair(args.private_key, args.public_key)
    print(f"Private key: {args.private_key}")
    print(f"Public key: {args.public_key}")
    return EXIT_SUCCESS


def _cmd_gui(_args: argparse.Namespace) -> int:
    from .gui import main as gui_main

    gui_main()
    return EXIT_SUCCESS


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="metaxtract",
        description="MetaXtract metadata scanner",
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    scan = sub.add_parser("scan", help="scan a file or folder and emit JSONL")
    scan.add_argument("path", help="file or folder to scan")
    scan.add_argument("--out", help="output JSONL path (default: stdout)")
    scan.add_argument(
        "--cache",
        choices=["on", "off"],
        default="on",
        help="enable/disable scan cache (default: on)",
    )
    scan.add_argument("--cache-dir", help="cache directory (default: .metaxtract_cache)")
    scan.add_argument(
        "--max-files",
        type=int,
        default=5000,
        help="maximum number of files to scan (default: 5000)",
    )
    scan.add_argument(
        "--include-hidden",
        action="store_true",
        help="include hidden files and directories",
    )
    scan.set_defaults(func=_cmd_scan)

    cache_cmd = sub.add_parser("cache", help="cache management commands")
    cache_cmd_sub = cache_cmd.add_subparsers(dest="cache_cmd", required=True)
    purge = cache_cmd_sub.add_parser("purge", help="remove all cached scan results")
    purge.add_argument("--cache-dir", help="cache directory (default: .metaxtract_cache)")
    purge.set_defaults(func=_cmd_cache_purge)

    report = sub.add_parser("report", help="build a report from a scan.jsonl (JSON or HTML)")
    report.add_argument("scan", help="input scan.jsonl")
    report.add_argument("--out", help="output report path (default: stdout)")
    report.add_argument("--format", choices=["json", "html"], help="report format")
    report.add_argument("--html", action="store_true", help="shortcut for --format html")
    report.set_defaults(func=_cmd_report)

    diff = sub.add_parser("diff", help="diff two scan.jsonl files")
    diff.add_argument("old", help="old scan.jsonl")
    diff.add_argument("new", help="new scan.jsonl")
    diff.add_argument("--out", help="output diff.json path (default: stdout)")
    diff.set_defaults(func=_cmd_diff)

    verify = sub.add_parser("verify", help="verify files on disk match hashes from scan.jsonl")
    verify.add_argument("scan", help="input scan.jsonl")
    verify.add_argument("base", help="base directory where files live")
    verify.set_defaults(func=_cmd_verify)

    verify_bundle = sub.add_parser("verify-bundle", help="verify a complete case ZIP bundle")
    verify_bundle.add_argument("bundle", help="case bundle ZIP path")
    verify_bundle.add_argument(
        "--files-base",
        help="verify external originals when the bundle does not include files",
    )
    verify_bundle.add_argument(
        "--public-key",
        help="Ed25519 public key used to verify a signed manifest",
    )
    verify_bundle.set_defaults(func=_cmd_verify_bundle)

    export_case = sub.add_parser(
        "export-case",
        help="create a case bundle containing manifest, hashes, report, and optional files",
    )
    export_case.add_argument("scan", help="input scan.jsonl")
    export_case.add_argument("out", help="output ZIP path")
    export_case.add_argument(
        "--include-files",
        action="store_true",
        help="include original files in the ZIP",
    )
    export_case.add_argument(
        "--files-base",
        help="base directory for originals (default: scan.jsonl directory)",
    )
    export_case.add_argument(
        "--redact",
        action="store_true",
        help="include redacted metadata outputs",
    )
    export_case.add_argument("--case-id", help="case identifier")
    export_case.add_argument("--notes", help="case notes")
    export_case.add_argument(
        "--signing-key",
        help="Ed25519 private key used to sign the manifest",
    )
    export_case.set_defaults(func=_cmd_export_case)

    keygen = sub.add_parser("keygen", help="create an Ed25519 signing key pair")
    keygen.add_argument("private_key", help="new private key PEM path")
    keygen.add_argument("public_key", help="new public key PEM path")
    keygen.set_defaults(func=_cmd_keygen)

    doctor = sub.add_parser("doctor", help="diagnose the environment and dependencies")
    doctor.set_defaults(func=_cmd_doctor)

    gui = sub.add_parser("gui", help="launch the desktop GUI")
    gui.set_defaults(func=_cmd_gui)

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.func(args))
    except (ImportError, OSError, ValueError, zipfile.BadZipFile) as exc:
        print(f"{args.cmd} failed: {exc}", file=sys.stderr)
        return EXIT_FAILURE


if __name__ == "__main__":
    raise SystemExit(main())
