#!/usr/bin/env python3
"""Compare unchanged-header property clients against original/native WIM libraries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
p.add_argument("--original", type=Path, default=Path("/tmp/wimlib-native-oracle/.libs"))
p.add_argument("--native", type=Path, required=True)
a = p.parse_args()
with tempfile.TemporaryDirectory(prefix="wim-properties-") as directory:
    directory = Path(directory)
    source = directory / "tree"
    source.mkdir()
    (source / "file").write_text("property client payload\n")
    fixture = directory / "two.wim"
    cli = a.original / "wimlib-imagex"
    env = dict(os.environ, LD_LIBRARY_PATH=str(a.original.resolve()))
    subprocess.run([str(cli), "capture", str(source), str(fixture), "First", "--compress=none"], check=True, stdout=subprocess.DEVNULL, env=env)
    subprocess.run([str(cli), "append", str(source), str(fixture), "Second"], check=True, stdout=subprocess.DEVNULL, env=env)
    outputs = []
    invalid_text = {}
    for name, library in (("original", a.original), ("native", a.native)):
        binary = directory / name
        subprocess.run(["cc", "-I" + str((a.source / "include").resolve()), "scripts/wimlib/probe-properties-api.c", "-L" + str(library.resolve()), "-Wl,-rpath," + str(library.resolve()), "-lwim", "-o", str(binary)], check=True)
        outputs.append(subprocess.check_output([str(binary), str(fixture)]))
        written = directory / (name + "-written.wim")
        invalid_text[name] = subprocess.check_output([str(binary), str(fixture), str(written)]).decode().splitlines()
        subprocess.run([str(cli), "verify", str(written)], check=True, stdout=subprocess.DEVNULL, env=env)
    if outputs[0] != outputs[1]:
        for index, (original, native) in enumerate(zip(outputs[0].splitlines(), outputs[1].splitlines()), 1):
            if original != native:
                print(f"line {index}: original={original!r}, native={native!r}")
        raise SystemExit("property ABI mismatch")
    if invalid_text["original"] != invalid_text["native"]:
        print(json.dumps(invalid_text, indent=2))
        raise SystemExit("raw platform text ABI mismatch")
    print(json.dumps({"exports": 9, "client_lines": len(outputs[0].splitlines()), "client_sha256": hashlib.sha256(outputs[0]).hexdigest(), "equal": True, "raw_platform_text": invalid_text, "raw_platform_text_equal": invalid_text["original"] == invalid_text["native"], "written_output_verified_by_original": True}, indent=2))
