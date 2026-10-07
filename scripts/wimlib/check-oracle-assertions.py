#!/usr/bin/env python3
"""Prove the capture comparator adapter fails when optional tree is unavailable."""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
parser.add_argument("--oracle", type=Path, required=True)
args = parser.parse_args()
results = {}
with tempfile.TemporaryDirectory(prefix="wimlib-assertion-") as temporary:
    root = Path(temporary)
    comparator = root / "tree-cmp"
    comparator.write_text("#!/usr/bin/env bash\nexit 1\n")
    comparator.chmod(0o755)
    run = root / "run"
    run.mkdir()
    for name, tree in (("original", args.source), ("strengthened", args.oracle)):
        text = (tree / "tests/test-imagex-capture_and_apply").read_text()
        start = text.index("do_tree_cmp() {")
        end = text.index("\n}\n", start) + 3
        # Exercise the unavailable optional diagnostic branch on every host.
        body = text[start:end].replace("[ -x /usr/bin/tree ]", "false")
        script = "error() { exit 42; }\n" + body + "\ndo_tree_cmp\n"
        result = subprocess.run(["bash", "-c", script], cwd=run, capture_output=True)
        results[name] = result.returncode
if results != {"original": 0, "strengthened": 42}:
    raise SystemExit(f"unexpected assertion behavior: {results}")
print(json.dumps(results, indent=2))
