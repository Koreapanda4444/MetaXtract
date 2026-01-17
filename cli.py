from __future__ import annotations

import argparse
import json
import sys
from typing import Callable, Iterable, Optional

import config
import utils

ExitCode = int

def _cmd_scan(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("scan: not implemented in commit0")
    return 2

def _cmd_report(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("report: not implemented in commit0")
    return 2

def _cmd_diff(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("diff: not implemented in commit0")
    return 2

def _cmd_sanitize(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("sanitize: not implemented in commit0")
    return 2

def _cmd_verify(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("verify: not implemented in commit0")
    return 2

def _cmd_gui(_: argparse.Namespace) -> ExitCode:
    utils.get_logger().error("gui: not implemented in commit0")
    return 2

def _cmd_version(_: argparse.Namespace) -> ExitCode:
    payload = {
        "name": "metaxtract",
        "version": utils.get_version(),
        "python": sys.version.split()[0],
    }
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    return 0

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="metaxtract", add_help=True)
    p.add_argument("-v", "--verbose", action="count", default=0)
    p.add_argument("--no-color", action="store_true")
    sub = p.add_subparsers(dest="command", required=True)

    sp = sub.add_parser("scan")
    sp.add_argument("path")
    sp.add_argument("--recursive", action="store_true")
    sp.set_defaults(_handler=_cmd_scan)

    sp = sub.add_parser("report")
    sp.add_argument("index")
    sp.set_defaults(_handler=_cmd_report)

    sp = sub.add_parser("diff")
    sp.add_argument("before")
    sp.add_argument("after")
    sp.set_defaults(_handler=_cmd_diff)

    sp = sub.add_parser("sanitize")
    sp.add_argument("path")
    sp.add_argument("--outdir", required=True)
    sp.set_defaults(_handler=_cmd_sanitize)

    sp = sub.add_parser("verify")
    sp.add_argument("index")
    sp.set_defaults(_handler=_cmd_verify)

    sp = sub.add_parser("gui")
    sp.set_defaults(_handler=_cmd_gui)

    sp = sub.add_parser("version")
    sp.set_defaults(_handler=_cmd_version)

    return p

def main(argv: Optional[Iterable[str]] = None) -> ExitCode:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    utils.configure_logging(int(args.verbose), bool(getattr(args, "no_color", False)))
    _ = config.Settings()
    handler: Callable[[argparse.Namespace], ExitCode] = getattr(args, "_handler")
    return int(handler(args))
