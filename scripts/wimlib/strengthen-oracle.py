#!/usr/bin/env python3
"""Fix two comparator assertion paths in a disposable upstream test copy."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("oracle", type=Path)
args = parser.parse_args()
changes = []
for relative in ("tests/test-imagex-capture_and_apply", "tests/test-imagex-ntfs"):
    path = args.oracle / relative
    original = path.read_text()
    lines = original.splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if "if ! ../tree-cmp " in line]
    if len(starts) != 1:
        raise SystemExit(f"unexpected comparator structure in {path}")
    start = starts[0]
    error_line = next(i for i in range(start, len(lines)) if "error 'Information was lost" in lines[i])
    if lines[error_line + 2].strip() != "fi" or lines[error_line + 3].strip() != "fi":
        raise SystemExit(f"unexpected error branch in {path}")
    # Close the optional diagnostics before the fatal assertion, not after it.
    assertion = [line[1:] for line in lines[error_line:error_line + 2]]
    replacement = [lines[error_line + 2], *assertion, lines[error_line + 3]]
    lines[error_line:error_line + 4] = replacement
    patched = "".join(lines)
    path.write_text(patched)
    changes.append({"path": relative,
                    "original_sha256": hashlib.sha256(original.encode()).hexdigest(),
                    "patched_sha256": hashlib.sha256(patched.encode()).hexdigest()})
print(json.dumps({"purpose": "unconditional comparator failure", "changes": changes}, indent=2))
