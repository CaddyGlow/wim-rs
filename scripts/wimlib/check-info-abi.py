#!/usr/bin/env python3
"""Compile one unchanged-header C client against original and native libraries."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
p.add_argument("--original", type=Path, default=Path("/tmp/wimlib-native-oracle/.libs"))
p.add_argument("--native", type=Path, required=True)
a = p.parse_args()
outputs = []
with tempfile.TemporaryDirectory(prefix="wim-info-abi-") as directory:
    for name, library in (("original", a.original), ("native", a.native)):
        binary = Path(directory) / name
        subprocess.run(["cc", "-I" + str((a.source / "include").resolve()),
                        "scripts/wimlib/probe-info-api.c", "-L" + str(library.resolve()),
                        "-Wl,-rpath," + str(library.resolve()), "-lwim", "-o", str(binary)], check=True)
        outputs.append(subprocess.check_output([str(binary)]))
if outputs[0] != outputs[1]:
    raise SystemExit("original/native C client outputs differ")
print(json.dumps({"exports": 4, "client_lines": len(outputs[0].splitlines()),
                  "client_sha256": hashlib.sha256(outputs[0]).hexdigest(), "equal": True}, indent=2))
