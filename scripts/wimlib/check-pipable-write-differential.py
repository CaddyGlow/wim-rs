#!/usr/bin/env python3
"""Native pipable archives: original seekable and fragmented stdin apply comparisons."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--oracle", type=Path, required=True)
p.add_argument("--native", type=Path, required=True)
a = p.parse_args()
oracle, native = str(a.oracle.resolve()), str(a.native.resolve())


def run(command):
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def tree(root):
    rows = []
    groups = {}
    for path in sorted(root.rglob("*")):
        info = path.lstat()
        name = path.relative_to(root).as_posix()
        mode = stat.S_IMODE(info.st_mode)
        if path.is_symlink():
            rows.append((name, "link", mode, os.readlink(path)))
        elif path.is_dir():
            rows.append((name, "dir", mode))
        elif path.is_file():
            rows.append((name, "file", mode, hashlib.sha256(path.read_bytes()).hexdigest()))
            groups.setdefault((info.st_dev, info.st_ino), []).append(name)
        else:
            raise AssertionError(f"unexpected test file type: {name}")
    return {"entries": rows, "hardlinks": sorted(sorted(g) for g in groups.values() if len(g) > 1)}


results = []
with tempfile.TemporaryDirectory(prefix="wim-pipable-diff-") as temp:
    root = Path(temp)
    source = root / "source"
    source.mkdir()
    (source / "nested").mkdir()
    (source / "nested" / "δ😀.bin").write_bytes(bytes(range(256)) * 600 + b"partial")
    (source / "empty").write_bytes(b"")
    (source / "small").write_bytes(b"first image")
    (source / "small").chmod(0o640)
    os.link(source / "small", source / "alias")
    (source / "symlink").symlink_to("nested/δ😀.bin")
    for codec in ("None", "XPRESS", "LZX", "LZMS"):
        for layout in ("ordinary", "pipable", "solid"):
            if codec == "None" and layout == "solid":
                continue
            input_wim = root / f"{codec}-{layout}.wim"
            options = [f"--compress={codec}", "--unix-data", "--nocheck"]
            if layout == "pipable":
                options.append("--pipable")
            if layout == "solid":
                options += ["--solid", f"--solid-compress={codec}"]
            run([oracle, "capture", str(source), str(input_wim), "First", *options])
            run([oracle, "append", str(source), str(input_wim), "Second", "--boot", "--unix-data"])
            expected = root / "original-applied"
            run([oracle, "apply", str(input_wim), "2", str(expected), "--unix-data"])
            expected_tree = tree(expected)
            # Reuse extraction destination safely through unique directories.
            for output_codec in ("None", "XPRESS", "LZX", "LZMS"):
                for integrity in (False, True):
                    output_wim = root / f"native-{codec}-{layout}-{output_codec}-{integrity}.wim"
                    run([native, str(input_wim), str(output_wim), "integrity" if integrity else "no-integrity", output_codec])
                    run([oracle, "verify", str(output_wim)])
                    for image in (1, 2):
                        actual = root / f"apply-{codec}-{layout}-{output_codec}-{integrity}-{image}"
                        run([oracle, "apply", str(output_wim), str(image), str(actual), "--unix-data"])
                        if tree(actual) != expected_tree:
                            raise AssertionError(f"tree differs: {codec}/{layout}/{integrity}/{image}")
                    data = output_wim.read_bytes()
                    for image in (1, 2):
                        destination = root / f"pipe-{codec}-{layout}-{output_codec}-{integrity}-{image}"
                        with tempfile.TemporaryFile() as logs:
                            process = subprocess.Popen([oracle, "apply", "-", str(image), str(destination), "--unix-data"],
                                                       stdin=subprocess.PIPE, stdout=logs, stderr=logs)
                            assert process.stdin is not None
                            try:
                                offset = 0
                                widths = (1, 7, 39, 207, 4093)
                                index = 0
                                while offset < len(data):
                                    width = widths[index % len(widths)]
                                    process.stdin.write(data[offset:offset+width])
                                    process.stdin.flush()
                                    offset += width
                                    index += 1
                            except BrokenPipeError:
                                # A pipe applier can terminate after consuming its final
                                # needed file resource, without reading seekable tables.
                                pass
                            finally:
                                try:
                                    process.stdin.close()
                                except BrokenPipeError:
                                    pass
                            status = process.wait()
                            if status:
                                logs.seek(0)
                                raise AssertionError(logs.read().decode(errors="replace"))
                        if tree(destination) != expected_tree:
                            raise AssertionError(f"pipe tree differs: {codec}/{layout}/{output_codec}/{integrity}/{image}")
                    if int.from_bytes(data[120:124], "little") != 2:
                        raise AssertionError("boot image index lost")
                    results.append({"codec": codec, "input_layout": layout, "output_codec": output_codec, "integrity": integrity,
                                    "images": 2, "seekable_and_pipe": True, "boot_index": 2, "output_sha256": hashlib.sha256(data).hexdigest()})
            # rglob destinations must be unique across source variants.
            expected.rename(root / f"original-{codec}-{layout}")
print(json.dumps({"cases": len(results), "results": results}, indent=2))
