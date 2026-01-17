from __future__ import annotations

import logging
import os
import sys
from typing import Optional

_VERSION = "0.0.0"
_LOGGER_NAME = "metaxtract"

class _PlainFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        level = record.levelname
        msg = record.getMessage()
        return f"{level}: {msg}"

class _AnsiFormatter(logging.Formatter):
    _COLORS = {
        "DEBUG": "\x1b[36m",
        "INFO": "\x1b[32m",
        "WARNING": "\x1b[33m",
        "ERROR": "\x1b[31m",
        "CRITICAL": "\x1b[35m",
    }
    _RESET = "\x1b[0m"

    def format(self, record: logging.LogRecord) -> str:
        level = record.levelname
        color = self._COLORS.get(level, "")
        msg = record.getMessage()
        if color:
            return f"{color}{level}{self._RESET}: {msg}"
        return f"{level}: {msg}"

def get_version() -> str:
    return _VERSION

def get_logger() -> logging.Logger:
    return logging.getLogger(_LOGGER_NAME)

def configure_logging(verbosity: int = 0, no_color: bool = False) -> None:
    logger = get_logger()
    logger.propagate = False
    logger.handlers.clear()

    if verbosity >= 2:
        level = logging.DEBUG
    elif verbosity == 1:
        level = logging.INFO
    else:
        level = logging.WARNING

    use_color = (not no_color) and _stdout_supports_color()
    formatter: logging.Formatter = _AnsiFormatter() if use_color else _PlainFormatter()

    h = logging.StreamHandler(stream=sys.stderr)
    h.setLevel(level)
    h.setFormatter(formatter)

    logger.setLevel(level)
    logger.addHandler(h)

def _stdout_supports_color() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    if not hasattr(sys.stderr, "isatty") or not sys.stderr.isatty():
        return False
    term = os.environ.get("TERM", "")
    if term.lower() in {"", "dumb"}:
        return False
    return True
