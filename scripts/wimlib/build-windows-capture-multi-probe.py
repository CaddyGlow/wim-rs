#!/usr/bin/env python3
"""Build the unchanged-header multi-ADD caller; guest execution is separate."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cc", required=True)
    parser.add_argument("--include", default="/tmp/wimlib/include")
    parser.add_argument("--library-dir", required=True)
    parser.add_argument("--output", default="/tmp/probe-windows-capture-multi.exe")
    parser.add_argument("--record", required=True)
    args = parser.parse_args()
    source = Path(__file__).with_name("probe-windows-capture-multi.c").resolve()
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [args.cc, "-municode", "-Wall", "-Wextra", "-Werror",
               f"-I{args.include}", str(source), "-o", str(output),
               "-ladvapi32", f"-L{args.library_dir}"]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    record = {"command": command, "exit_code": result.returncode,
              "stdout": result.stdout, "stderr": result.stderr,
              "source_sha256": sha(source),
              "original_header_sha256": sha(Path(args.include) / "wimlib.h"),
              "owned_guest_fixture": r"C:\wim-capture-multi-20261003",
              "guest_arguments": ["<dll>", "<scenario 0..4>", "<output-wim or ->"],
              "scenarios": {"0": "same update: ACL then NO_ACLS",
                            "1": "same update: NO_ACLS then ACL",
                            "2": "same update: source DACL changes after first scan",
                            "3": "separate updates: ACL then NO_ACLS",
                            "4": "separate updates: source DACL changes before second"}}
    if result.returncode == 0:
        record["probe_sha256"] = sha(output)
    destination = Path(args.record)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(record, indent=2) + "\n")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
