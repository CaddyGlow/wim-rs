#!/usr/bin/env python3
"""Print a content-addressed upstream test and fixture inventory."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
args = parser.parse_args()
paths = []
for directory in ("tests", "tools/libFuzzer"):
    paths.extend(path for path in (args.source / directory).rglob("*") if path.is_file())
paths.extend(args.source / path for path in
             ("Makefile.am", "tools/test-examples.sh", "tools/msvc-test-examples.bat"))
files = []
for path in sorted(paths):
    raw = path.read_bytes()
    files.append({"path": str(path.relative_to(args.source)), "bytes": len(raw),
                  "sha256": hashlib.sha256(raw).hexdigest()})
print(json.dumps({"source": str(args.source), "files": files}, indent=2))
