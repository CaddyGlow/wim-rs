#!/usr/bin/env python3
"""Capture with C, read hash-identified payload with Rust, compare exact bytes."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--oracle", type=Path, required=True)
parser.add_argument("--native", type=Path, required=True)
args = parser.parse_args()
payload = bytes(range(256)) * 600 + b"partial last chunk"
digest = hashlib.sha1(payload).hexdigest()
results = []
with tempfile.TemporaryDirectory(prefix="wim-archive-differential-") as directory:
    root = Path(directory)
    source = root / "source"
    source.mkdir()
    (source / "payload.bin").write_bytes(payload)
    for codec in ("None", "XPRESS", "LZX", "LZMS"):
        for layout in ("ordinary", "pipable", "solid"):
            if codec == "None" and layout == "solid":
                continue
            options = [f"--compress={codec}", "--no-acls", "--nocheck"]
            if layout == "pipable": options.append("--pipable")
            if layout == "solid": options += ["--solid", f"--solid-compress={codec}"]
            archive = root / f"{codec}-{layout}.wim"
            subprocess.run([str(args.oracle.resolve()), "capture", str(source), str(archive), *options],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            output = root / "native-output"
            subprocess.run([str(args.native.resolve()), str(archive), digest, str(output)], check=True)
            if output.read_bytes() != payload:
                raise SystemExit(f"payload differs for {codec}/{layout}")
            results.append({"codec": codec, "layout": layout, "bytes": len(payload),
                            "payload_sha256": hashlib.sha256(payload).hexdigest(),
                            "wim_sha256": hashlib.sha256(archive.read_bytes()).hexdigest()})
print(json.dumps({"cases": len(results), "results": results}, indent=2))
