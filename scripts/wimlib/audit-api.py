#!/usr/bin/env python3
"""Snapshot public declarations, or check documentation coverage against source."""
import argparse
import hashlib
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
parser.add_argument("--check", type=Path)
args = parser.parse_args()
header = args.source / "include/wimlib.h"
raw = header.read_bytes()
text = raw.decode()
functions = []
for match in re.finditer(r"^wimlib_[a-z_]+\([^;]+;", text, re.M):
    declaration = match.group()
    functions.append({"name": declaration.split("(")[0],
                      "line": text.count("\n", 0, match.start()) + 1,
                      "declaration": declaration})
if args.check:
    documented = args.check.read_text()
    missing = [f["name"] for f in functions
               if not re.search(r"\b" + re.escape(f["name"]) + r"\b", documented)]
    constants = sorted(set(re.findall(r"\bWIMLIB_[A-Z0-9_]+\b", text)))
    missing_constants = [name for name in constants
                         if not re.search(r"\b" + re.escape(name) + r"\b", documented)]
    print(json.dumps({"functions": len(functions), "missing": missing,
                      "constant_identifiers": len(constants),
                      "missing_constants": missing_constants}, indent=2))
    raise SystemExit(bool(missing or missing_constants))
print(json.dumps({"source": str(header), "sha256": hashlib.sha256(raw).hexdigest(),
                  "functions": functions}, indent=2))
