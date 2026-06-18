#!/usr/bin/env python3
"""Fail if any tracked text source contains a NUL byte or other control char.

A literal NUL byte pasted into a source file compiles fine but makes git, grep,
and ripgrep treat the whole file as binary, so code search silently returns no
matches for it (issue #295 was exactly this: two stray NULs in a Rust comment).
Other C0 control characters are the same class of paste artifact. This guard runs
in CI to catch them at PR time instead of after they quietly break code search.

It cannot be done with `git grep`: the shell cannot pass a NUL on the command
line, and git would classify the offending file as binary and skip it anyway. So
this reads the raw bytes of every tracked, non-binary file itself.

Run from the repository root:

    python3 scripts/check-control-chars.py

Exits 0 when clean, 1 (with a report of each offending file) when not.
"""

from __future__ import annotations

import os
import subprocess
import sys

# Control bytes that are legitimate in text: tab, line feed, carriage return.
_ALLOWED_CONTROL_BYTES = frozenset({0x09, 0x0A, 0x0D})

# Extensions for files that are binary by nature, so control bytes are expected
# and must not be flagged. Everything else tracked is treated as text and scanned,
# including extensionless scripts (Dockerfiles, the workspace `seraphim-*` helpers).
_BINARY_EXTENSIONS = frozenset(
    {
        b".wav",
        b".mp3",
        b".mp4",
        b".png",
        b".jpg",
        b".jpeg",
        b".gif",
        b".ico",
        b".webp",
        b".bmp",
        b".pdf",
        b".woff",
        b".woff2",
        b".ttf",
        b".otf",
        b".eot",
        b".zip",
        b".gz",
        b".bz2",
        b".xz",
        b".tar",
        b".bin",
        b".wasm",
    }
)

# Vendored third-party tooling that is not our source. Yarn Berry commits its
# release bundle here (a minified bundle that embeds control bytes inside string
# literals), and we neither author nor edit it, so it is out of scope for this guard.
_EXCLUDED_PREFIXES = (b".yarn/",)


def _tracked_files() -> list[bytes]:
    """Return every tracked file path as raw bytes (NUL-delimited from git)."""
    # `-z` keeps paths intact even with newlines or non-UTF-8 bytes in them, and
    # lets us read each file by its exact bytes regardless of locale.
    output = subprocess.run(
        ["git", "ls-files", "-z"],
        check=True,
        capture_output=True,
    ).stdout
    return [path for path in output.split(b"\x00") if path]


def _is_scanned(path: bytes) -> bool:
    """Whether a tracked path should be scanned (a text source, not vendored/binary)."""
    if path.startswith(_EXCLUDED_PREFIXES):
        return False
    _, extension = os.path.splitext(path)
    return extension.lower() not in _BINARY_EXTENSIONS


def _first_violation(data: bytes) -> tuple[int, int] | None:
    """Return (offset, byte) of the first disallowed control byte, or None if clean."""
    for offset, byte in enumerate(data):
        # DEL (0x7F) and the C0 controls below 0x20, minus the allowed whitespace.
        if byte == 0x7F or (byte < 0x20 and byte not in _ALLOWED_CONTROL_BYTES):
            return offset, byte
    return None


def main() -> int:
    """Scan every tracked text source and report any control-byte contamination."""
    failures: list[str] = []
    for path in _tracked_files():
        if not _is_scanned(path):
            continue
        with open(path, "rb") as handle:
            data = handle.read()
        violation = _first_violation(data)
        if violation is None:
            continue
        offset, byte = violation
        line = data.count(b"\n", 0, offset) + 1
        column = offset - data.rfind(b"\n", 0, offset)
        label = "NUL" if byte == 0x00 else f"0x{byte:02x}"
        display_path = os.fsdecode(path)
        failures.append(
            f"{display_path}:{line}:{column}: disallowed control byte {label} "
            f"(byte offset {offset})"
        )

    if failures:
        print("Control-byte check failed: source files must not contain NUL bytes")
        print("or other control characters (tab, newline, and carriage return are")
        print("allowed). These are usually paste artifacts that silently break code")
        print("search (issue #295). Offending files:")
        print()
        for failure in failures:
            print(f"  {failure}")
        return 1

    print("Control-byte check passed: no NUL bytes or stray control characters found.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
