
from __future__ import annotations
import bundle_export
import inspect

def _call_with_matching_kwargs(fn, ns):
    sig = inspect.signature(fn)
    kwargs = {k: v for k, v in vars(ns).items() if k in sig.parameters}
    return fn(**kwargs)

def _cmd_export_case(args) -> int:
    candidates = [
        "export_case_bundle",
        "export_case",
        "export",
        "main",
    ]
    for name in candidates:
        fn = getattr(bundle_export, name, None)
        if callable(fn):
            try:
                result = _call_with_matching_kwargs(fn, args)
            except TypeError:
                result = fn()
            return 0 if result is None else int(result)
    raise SystemExit("bundle_export: no export function found")

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Callable, Iterable, Optional

import config
import diff_report
import engine
import extract_common
import gui
import index_store
import report
import sanitize
import utils
import verify

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


def _stdout_write(text: str) -> None:
    try:
        sys.stdout.write(text)
    except BrokenPipeError:
        try:
            sys.stdout.close()
        except Exception:
            pass
        os._exit(0)

def _cmd_scan(_: argparse.Namespace) -> ExitCode:
    args = _
    include_exts = extract_common.parse_include_extensions(getattr(args, "include", None))
    exclude_patterns = extract_common.split_patterns(getattr(args, "exclude", []) or [])
    hash_algo = str(getattr(args, "hash", "none") or "none")
    out_path = getattr(args, "out", None)
    redact = bool(getattr(args, "redact", False))
    threads = int(getattr(args, "threads", 1) or 1)
    if threads < 1:
        threads = 1

    cancel = utils.CancelToken()

    def _scan_records() -> Iterable[dict]:
        return engine.scan_iter(
            str(args.path),
            recursive=bool(getattr(args, "recursive", False)),
            include_exts=include_exts,
            exclude_patterns=exclude_patterns,
            hash_algo=hash_algo,
            redact=redact,
            threads=threads,
            cancel=cancel,
        )

    cancelled = False
    total = 0
    file_error_files = 0
    error_code_counts: dict[str, int] = {}

    try:
        # enumerate 에러는 레코드로 만들 수 없으므로 먼저 수집
        enum = engine.enumerate_files(
            str(args.path),
            recursive=bool(getattr(args, "recursive", False)),
            include_exts=include_exts,
            exclude_patterns=exclude_patterns,
        )
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"스캔 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)

    is_single_file = False
    try:
        is_single_file = Path(str(args.path)).is_file()
    except Exception:
        is_single_file = False

    if is_single_file:
        # 단일 파일은 1개만 출력
        try:
            rec = next(iter(_scan_records()))
        except StopIteration:
            raise utils.ProcessingError("스캔 결과가 비어 있습니다.", exit_code=utils.ExitCodes.FAILURE)
        except KeyboardInterrupt:
            cancel.cancel()
            cancelled = True
            raise utils.ProcessingError("사용자에 의해 중단되었습니다.", exit_code=utils.ExitCodes.FAILURE)

        if out_path:
            header = engine.make_session_header(
                root=str(args.path),
                recursive=bool(getattr(args, "recursive", False)),
                include=getattr(args, "include", None),
                exclude=exclude_patterns,
                hash_algo=hash_algo,
                threads=threads,
            )
            index_store.write_jsonl(str(out_path), [header, rec], append=False)
        else:
            _stdout_write(json.dumps(rec, ensure_ascii=False) + "\n")

        raw = rec.get("raw") if isinstance(rec, dict) else None
        errs = raw.get("errors") if isinstance(raw, dict) else None
        if isinstance(errs, list) and errs:
            return utils.ExitCodes.FAILURE
        return utils.ExitCodes.SUCCESS

    if out_path:
        header = engine.make_session_header(
            root=str(args.path),
            recursive=bool(getattr(args, "recursive", False)),
            include=getattr(args, "include", None),
            exclude=exclude_patterns,
            hash_algo=hash_algo,
            threads=threads,
        )
        try:
            with index_store.JsonlWriter(str(out_path), append=False) as w:
                w.write(header)

                gen = _scan_records()
                try:
                    for rec in gen:
                        total += 1
                        raw = rec.get("raw") if isinstance(rec, dict) else None
                        errs = raw.get("errors") if isinstance(raw, dict) else None
                        if isinstance(errs, list) and errs:
                            file_error_files += 1
                            for it in errs:
                                if isinstance(it, dict):
                                    code = str(it.get("error_code") or "unknown")
                                    error_code_counts[code] = error_code_counts.get(code, 0) + 1

                        w.write(rec)
                except KeyboardInterrupt:
                    cancel.cancel()
                    cancelled = True
                    try:
                        if hasattr(gen, "close"):
                            gen.close()  # type: ignore[attr-defined]
                    except Exception:
                        pass
        except OSError as e:
            raise utils.ProcessingError(f"인덱스 저장 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)
    else:
        gen = _scan_records()
        try:
            for rec in gen:
                total += 1
                raw = rec.get("raw") if isinstance(rec, dict) else None
                errs = raw.get("errors") if isinstance(raw, dict) else None
                if isinstance(errs, list) and errs:
                    file_error_files += 1
                    for it in errs:
                        if isinstance(it, dict):
                            code = str(it.get("error_code") or "unknown")
                            error_code_counts[code] = error_code_counts.get(code, 0) + 1
                _stdout_write(json.dumps(rec, ensure_ascii=False) + "\n")
        except KeyboardInterrupt:
            cancel.cancel()
            cancelled = True
            try:
                if hasattr(gen, "close"):
                    gen.close()  # type: ignore[attr-defined]
            except Exception:
                pass

    # enumerate 단계 오류는 stderr로
    if enum.errors:
        max_lines = 20
        for msg in enum.error_messages[:max_lines]:
            utils.error(msg)
        remaining = enum.errors - min(enum.errors, max_lines)
        if remaining > 0:
            utils.error(f"오류 메시지 {remaining}건은 생략되었습니다")

    # 최종 요약(구조화)
    if error_code_counts:
        top = sorted(error_code_counts.items(), key=lambda x: (-x[1], x[0]))
        top_text = ", ".join([f"{k}={v}" for k, v in top[:10]])
    else:
        top_text = ""

    utils.get_logger().info(
        "scan summary: files=%s file_errors=%s enum_errors=%s cancelled=%s threads=%s %s",
        total,
        file_error_files,
        int(enum.errors),
        bool(cancelled),
        threads,
        ("codes=" + top_text) if top_text else "",
    )

    if cancelled or enum.errors or file_error_files:
        utils.get_logger().warning(
            "scan finished with issues: files=%s failed_files=%s enum_errors=%s cancelled=%s %s",
            total,
            file_error_files,
            int(enum.errors),
            bool(cancelled),
            ("codes=" + top_text) if top_text else "",
        )

    if cancelled:
        return utils.ExitCodes.FAILURE
    if enum.errors or file_error_files:
        return utils.ExitCodes.FAILURE
    return utils.ExitCodes.SUCCESS

def _cmd_report(_: argparse.Namespace) -> ExitCode:
    args = _
    fmt = str(getattr(args, "format", "txt") or "txt").lower()
    template = str(getattr(args, "template", "privacy") or "privacy").lower()
    redact = bool(getattr(args, "redact", False))
    index_path = str(getattr(args, "index"))

    try:
        out = report.generate(index_path, fmt=fmt, template=template, redact=redact)
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except ValueError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.USAGE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"리포트 생성 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)

    _stdout_write(out)
    return utils.ExitCodes.SUCCESS

def _cmd_diff(_: argparse.Namespace) -> ExitCode:
    args = _
    before_path = str(getattr(args, "before"))
    after_path = str(getattr(args, "after"))
    key_field = str(getattr(args, "key", "path") or "path")
    out_path = getattr(args, "out", None)
    
    # key_field 검증
    if key_field not in ["path", "sha256"]:
        raise utils.ProcessingError(
            f"잘못된 key 옵션입니다: {key_field} (가능한 값: path, sha256)",
            exit_code=utils.ExitCodes.USAGE,
        )
    
    try:
        # 출력 형식 결정
        fmt = "txt"
        if out_path:
            if out_path.endswith(".json"):
                fmt = "json"
            elif out_path.endswith(".txt"):
                fmt = "txt"
        
        out = diff_report.generate(
            before_path,
            after_path,
            key_field=key_field,
            fmt=fmt,
        )
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except ValueError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.USAGE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"Diff 생성 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)
    
    # 출력
    if out_path:
        Path(out_path).write_text(out, encoding="utf-8")
    else:
        _stdout_write(out)
    
    return utils.ExitCodes.SUCCESS

def _cmd_sanitize(_: argparse.Namespace) -> ExitCode:
    args = _
    in_path = str(getattr(args, "path"))
    outdir = str(getattr(args, "outdir"))
    mode = str(getattr(args, "mode", "redact") or "redact").lower()

    if mode not in {"redact", "minimal"}:
        raise utils.ProcessingError(
            f"잘못된 mode 옵션입니다: {mode} (가능한 값: redact, minimal)",
            exit_code=utils.ExitCodes.USAGE,
        )

    try:
        summary = sanitize.sanitize_path(in_path, outdir, mode=mode)  # type: ignore[arg-type]
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"sanitize 처리 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)

    utils.get_logger().info(
        "sanitize done: processed=%s copied=%s errors=%s outdir=%s",
        summary.processed,
        summary.copied,
        summary.errors,
        outdir,
    )
    return utils.ExitCodes.SUCCESS

def _cmd_verify(_: argparse.Namespace) -> ExitCode:
    args = _
    index_path = str(getattr(args, "index"))
    summary = bool(getattr(args, "summary", False))
    summary_out = getattr(args, "summary_out", None)

    try:
        res = verify.verify_index(index_path, summary=summary)
    except FileNotFoundError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.FAILURE, cause=e)
    except ValueError as e:
        raise utils.ProcessingError(str(e), exit_code=utils.ExitCodes.USAGE, cause=e)
    except OSError as e:
        raise utils.ProcessingError(f"verify 처리 중 오류가 발생했습니다: {e}", exit_code=utils.ExitCodes.FAILURE, cause=e)

    if not res.ok:
        max_lines = 40
        for issue in res.issues[:max_lines]:
            line_part = f"L{issue.line}" if issue.line > 0 else "(header)"
            utils.error(f"{index_path}:{line_part} {issue.code}: {issue.message}")
        remaining = len(res.issues) - min(len(res.issues), max_lines)
        if remaining > 0:
            utils.error(f"검증 이슈 {remaining}건은 생략되었습니다")
        return utils.ExitCodes.FAILURE

    if summary:
        out = verify.format_summary_json(res) + "\n"
        if summary_out:
            Path(str(summary_out)).write_text(out, encoding="utf-8")
        else:
            _stdout_write(out)
    return utils.ExitCodes.SUCCESS

def _cmd_gui(_: argparse.Namespace) -> ExitCode:
    args = _
    index_path = getattr(args, "index", None)
    try:
        gui.launch(str(index_path) if index_path else None)
    except utils.ProcessingError:
        raise
    except Exception as e:
        raise utils.ProcessingError("GUI 실행 중 오류가 발생했습니다.", exit_code=utils.ExitCodes.FAILURE, cause=e)
    return utils.ExitCodes.SUCCESS

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

    sp = sub.add_parser("export-case")
    sp.add_argument("index", help="스캔 결과 index.jsonl")
    sp.add_argument("--out", required=True, help="내보낼 zip 파일 경로")
    sp.add_argument("--include-original", action="store_true", help="원본 파일 포함")
    sp.add_argument("--exclude-original", action="store_true", help="원본 파일 제외(기본)")
    sp.add_argument("--sanitize-logs", default=None, help="sanitize 로그 폴더 경로 포함")
    sp.set_defaults(_handler=_cmd_export_case)
    sp.add_argument("--include", default=None)
    sp.add_argument("--exclude", action="append", default=[])
    sp.add_argument("--hash", choices=["sha256", "md5", "none"], default="none")
    sp.add_argument("--threads", type=int, default=config.Settings().threads, help="스캔 병렬 스레드 수")
    sp.add_argument("--redact", action="store_true")
    sp.add_argument("--out", default=None)
    sp.set_defaults(_handler=_cmd_scan)

    sp = sub.add_parser("report")
    sp.add_argument("index")
    sp.add_argument("--format", choices=["json", "csv", "txt"], default="txt")
    sp.add_argument("--template", choices=["privacy", "forensics", "content"], default="privacy")
    sp.add_argument("--redact", action="store_true")
    sp.set_defaults(_handler=_cmd_report)

    sp = sub.add_parser("diff")
    sp.add_argument("before", help="비교 기준 인덱스 파일 (before)")
    sp.add_argument("after", help="비교 대상 인덱스 파일 (after)")
    sp.add_argument("--key", choices=["path", "sha256"], default="path", 
                    help="비교 키 필드 (기본: path)")
    sp.add_argument("--out", default=None, 
                    help="출력 파일 (.txt 또는 .json). 미지정 시 stdout")
    sp.set_defaults(_handler=_cmd_diff)

    sp = sub.add_parser("sanitize")
    sp.add_argument("path")
    sp.add_argument("--outdir", required=True)
    sp.add_argument("--mode", choices=["redact", "minimal"], default="redact")
    sp.set_defaults(_handler=_cmd_sanitize)

    sp = sub.add_parser("verify")
    sp.add_argument("index")
    sp.add_argument("--summary", action="store_true", help="인덱스 digest/요약을 JSON으로 출력")
    sp.add_argument("--summary-out", default=None, help="summary JSON을 파일로 저장")
    sp.set_defaults(_handler=_cmd_verify)

    sp = sub.add_parser("gui")
    sp.add_argument("index", nargs="?", default=None, help="로드할 JSONL 인덱스 파일(미지정 시 파일 선택)")
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
