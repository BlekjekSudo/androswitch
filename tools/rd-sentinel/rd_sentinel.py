#!/usr/bin/env python3
"""Executable wrapper for RD Sentinel."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from sentinel.cli import main  # noqa: E402


if __name__ == "__main__":
    raise SystemExit(main())
