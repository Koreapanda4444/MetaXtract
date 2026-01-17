from __future__ import annotations

import argparse
import json
import sys
from typing import Callable, Iterable, Optional

import config
import engine
import extract_common
import utils

ExitCode = int


class _MetaXtractArgumentParser(argparse.ArgumentParser):
    def error(self, message: str) -> None:
        raise utils.UsageError(message)


def _extract_global_flags(argv: list[str]) -> tuple[int, bool]:
    verbosity = 0
    no_color = False
    for token in argv:
        if token == "--no-color":
            no_color = True
            continue
        if token == "--verbose" or token == "-v":
            verbosity += 1
            continue
        if token.startswith("-v") and len(token) > 2 and set(token[1:]) == {"v"}:
            verbosity += (len(token) - 1)
            continue
    return verbosity, no_color


def _extract_command_token(argv: list[str]) -> Optional[str]:
    for token in argv:
        if token == "--no-color":
            continue
        if token in {"--verbose", "-v"}:
            continue
        if token.startswith("-v") and len(token) > 2 and set(token[1:]) == {"v"}:
            continue
        if token.startswith("-"):
            continue
        return token
    return None


def _get_subparser(parser: argparse.ArgumentParser, command: str) -> Optional[argparse.ArgumentParser]:
    for action in getattr(parser, "_actions", []):
        if isinstance(action, argparse._SubParsersAction):
            return action.choices.get(command)
    return None

def _cmd_scan(_: argparse.Namespace) -> ExitCode:
    args = _
    include_exts = extract_common.parse_include_extensions(getattr(args, "include", None))
    exclude_patterns = extract_common.split_patterns(getattr(args, "exclude", []) or [])

    try:
        result = engine.enumerate_files(
            str(args.path),
            recursive=bool(getattr(args, "recursive", False)),
            include_exts=include_exts,
            exclude_patterns=exclude_patterns,
        )
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"스캔 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)

    sys.stdout.write(f"found={result.found}\n")
    sys.stdout.write(f"excluded={result.excluded}\n")
    sys.stdout.write(f"errors={result.errors}\n")
    for path in result.files:
        sys.stdout.write(path + "\n")

    if result.errors:
        max_lines = 20
        for msg in result.error_messages[:max_lines]:
            utils.error(msg)
        remaining = result.errors - min(result.errors, max_lines)
        if remaining > 0:
            utils.error(f"오류 메시지 {remaining}건은 생략되었습니다")
        return utils.ExitCodes.FAILURE

    return utils.ExitCodes.SUCCESS

def _cmd_report(_: argparse.Namespace) -> ExitCode:
    return utils.not_implemented("report")

def _cmd_diff(_: argparse.Namespace) -> ExitCode:
    return utils.not_implemented("diff")

def _cmd_sanitize(_: argparse.Namespace) -> ExitCode:
    return utils.not_implemented("sanitize")

def _cmd_verify(_: argparse.Namespace) -> ExitCode:
    return utils.not_implemented("verify")

def _cmd_gui(_: argparse.Namespace) -> ExitCode:
    return utils.not_implemented("gui")

def _cmd_version(_: argparse.Namespace) -> ExitCode:
    payload = {
        "name": "metaxtract",
        "version": utils.get_version(),
        "python": sys.version.split()[0],
    }
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    return 0

def build_parser() -> argparse.ArgumentParser:
    p = _MetaXtractArgumentParser(
        prog="metaxtract",
        add_help=True,
        description="MetaXtract CLI (WIP)",
        epilog=(
            "Exit codes: 0=success, 2=usage/not-implemented, 3=processing failure. "
            "Verbose: -v=INFO, -vv(2+)=DEBUG"
        ),
    )
    p.add_argument(
        "-v",
        "--verbose",
        action="count",
        default=0,
        help="Increase verbosity (0=WARNING, 1=INFO, 2+=DEBUG).",
    )
    p.add_argument("--no-color", action="store_true", help="Disable ANSI colors on stderr.")
    sub = p.add_subparsers(dest="command", required=True)

    sp = sub.add_parser("scan")
    sp.add_argument("path")
    sp.add_argument("--recursive", action="store_true")
    sp.add_argument("--include", default=None)
    sp.add_argument("--exclude", action="append", default=[])
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
    argv_list = list(argv) if argv is not None else sys.argv[1:]

    pre_verbose, pre_no_color = _extract_global_flags(argv_list)
    utils.configure_logging(pre_verbose, pre_no_color)

    parser = build_parser()
    try:
        args = parser.parse_args(argv_list)
    except utils.UsageError as e:
        utils.error(e.user_message)
        cmd = _extract_command_token(argv_list)
        subparser = _get_subparser(parser, cmd) if cmd else None
        (subparser or parser).print_help(file=sys.stderr)
        return utils.ExitCodes.USAGE
    except SystemExit as e:
        try:
            return int(e.code)
        except Exception:
            return utils.ExitCodes.USAGE

    utils.configure_logging(int(args.verbose), bool(getattr(args, "no_color", False)))
    _ = config.Settings()
    handler: Callable[[argparse.Namespace], ExitCode] = getattr(args, "_handler")

    try:
        return int(handler(args))
    except utils.ProcessingError as e:
        return utils.fail(e.user_message, code=e.exit_code, exc=e)
    except Exception as e:
        return utils.fail("예기치 못한 내부 오류가 발생했습니다.", code=utils.ExitCodes.INTERNAL_ERROR, exc=e)
