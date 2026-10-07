#!/usr/bin/env python3
"""Unchanged-header extraction oracle using disposable Linux filesystem targets."""
import argparse
import hashlib
import json
import os
import pathlib
import stat
import subprocess
import tempfile


def snapshot(root, all_images=False, aborted=False):
    root = os.fsencode(root)
    result = []
    inodes = {}

    def visit(path, relative):
        info = os.lstat(path)
        key = (info.st_dev, info.st_ino)
        kind = stat.S_IFMT(info.st_mode)
        entry = {"path": relative.hex(), "mode": info.st_mode, "uid": info.st_uid,
                 "gid": info.st_gid, "mtime": info.st_mtime_ns, "atime": info.st_atime_ns}
        if aborted:
            for field in ["mtime", "atime"]:
                if entry[field] > 1_600_000_002_000_000_000:
                    entry[field] = "unrestored filesystem creation time"
        if not relative and all_images:
            entry.pop("mtime")
            entry.pop("atime")
        if kind != stat.S_IFDIR:
            entry["inode_group"] = inodes.setdefault(key, len(inodes))
        if kind == stat.S_IFREG:
            with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOATIME), "rb") as stream:
                entry["sha1"] = hashlib.sha1(stream.read()).hexdigest()
            entry["size"] = info.st_size
        elif kind == stat.S_IFLNK:
            entry["target"] = os.readlink(path).replace(root, b"<TARGET>").hex()
        entry["xattrs"] = {os.fsencode(name).hex(): os.getxattr(path, name, follow_symlinks=False).hex()
                           for name in os.listxattr(path, follow_symlinks=False)}
        result.append(entry)
        if kind == stat.S_IFDIR:
            for name in sorted(os.listdir(path)):
                visit(path + b"/" + name, relative + b"/" + name)

    if os.path.lexists(root):
        visit(root, b"")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--original", default="/tmp/wimlib-native-oracle")
    parser.add_argument("--native", default="target/debug")
    parser.add_argument("--output", default="docs/wimlib/evidence/native-ffi-extract/differential.json")
    args = parser.parse_args()
    repo = pathlib.Path.cwd()
    env = {**os.environ, "WIMLIB_DISABLE_CPU_FEATURES": "sse4.2"}
    with tempfile.TemporaryDirectory(prefix="wim-extract-differential-") as temporary:
        temp = pathlib.Path(temporary)
        probes = []
        for label, library in [("original", pathlib.Path(args.original) / ".libs"),
                               ("native", pathlib.Path(args.native))]:
            library = library.resolve()
            probe = temp / ("probe-" + label)
            subprocess.run(["cc", "scripts/wimlib/probe-extract-api.c", "-I/tmp/wimlib/include",
                            "-L" + str(library), "-Wl,-rpath," + str(library), "-lwim", "-o", str(probe)], check=True)
            probes.append(probe)
        source = temp / "source"
        source.mkdir()
        (source / "nested").mkdir()
        (source / "nested" / "data").write_bytes(b"payload" * 9999)
        (source / "empty").touch()
        os.link(source / "nested" / "data", source / "alias")
        os.symlink("nested/data", source / "relative")
        os.symlink(str(source / "nested" / "data"), source / "absolute")
        os.symlink("a\\backslash", source / "backslash-link")
        os.mkfifo(source / "fifo")
        os.chmod(source / "nested" / "data", 0o751)
        os.setxattr(source / "nested" / "data", "user.probe", b"value\x00bytes")
        (source / "case").write_bytes(b"lower")
        (source / "CASE").write_bytes(b"upper")
        (source / "unicode-😀").touch()
        with open(os.fsencode(source) + b"/surrogate-\xed\xa0\x80", "wb") as stream:
            stream.write(b"WTF-8 filename")
        (source / "identical-data").write_bytes((source / "nested" / "data").read_bytes())
        with open(source / "sparse", "wb") as stream:
            stream.write(b"head")
            stream.seek(512 * 1024)
            stream.write(b"tail")
        os.link(source / "empty", source / "empty-alias")
        for path in [source, *source.rglob("*")]:
            os.utime(path, ns=(1_600_000_000_123_456_700, 1_600_000_001_765_432_100), follow_symlinks=False)
        original = pathlib.Path(args.original) / "wimlib-imagex"
        wims = []
        for codec in ["none", "XPRESS", "LZX", "LZMS"]:
            path = temp / (codec + ".wim")
            subprocess.run([str(original), "capture", str(source), str(path), "Fixture", "--unix-data",
                            "--compress=" + codec], env=env, stdout=subprocess.DEVNULL, check=True)
            wims.append(path)
        cases = [(wim, 1, flags, status, existing) for wim in wims
                 for flags, status, existing in [(0,0,False),(32,0,False),(32,0,True),(512,0,False),(0,1,False),(0,2,False),
                                                  (0,103,False),(0,104,False),(0,106,False),(0,107,False),(0,204,False)]]
        cases.extend((wims[0], image, flags, 0, False) for image in [0,2,-1] for flags in [0,1,0x400,0x80,0xc0,0x300,0x400000,0x1000000,4])
        # Execute the original named-case fixture commands unchanged. The shell
        # functions only allocate fresh inputs beneath this disposable directory.
        common = temp / "common"
        common.mkdir()
        subprocess.run(["bash", "-c", '''
            fixture_index=0
            msg() { fixture_label="$*"; }
            do_test() {
                fixture="$fixture_root/case-$fixture_index"
                mkdir "$fixture"
                (cd "$fixture"; eval "$1") || exit 1
                printf '%s\\t%s\\n' "$fixture_index" "$fixture_label" >> "$fixture_root/cases.tsv"
                fixture_index=$((fixture_index + 1))
            }
            source "$srcdir/tests/common_tests.sh"
        '''], env={**env,"fixture_root":str(common),"srcdir":"/tmp/wimlib"}, check=True)
        named_cases = []
        for line in (common / "cases.tsv").read_text().splitlines():
            index, label = line.split("\t",1)
            named_cases.append(label)
            for codec in ["none", "XPRESS", "LZX"]:
                path = temp / ("named-" + index + "-" + codec + ".wim")
                subprocess.run([str(original),"capture",str(common / ("case-" + index)),str(path),label,"--norpfix","--compress=" + codec],env=env,stdout=subprocess.DEVNULL,check=True)
                cases.append((path,1,0,0,False))
        for fixture in pathlib.Path("/tmp/wimlib/tests/wims").glob("*.wim"):
            cases.append((fixture,1,32 if fixture.stem == "linux_xattrs_old" else 0,0,False))
            if fixture.stem.startswith("corrupted_file"):
                cases.append((fixture,1,2,0,False))
        split = temp / "split.swm"
        subprocess.run([str(original),"split",str(wims[0]),str(split),"0.1"],env=env,stdout=subprocess.DEVNULL,check=True)
        parts = sorted(temp.glob("split*.swm"))
        cases.append((split,1,32,0,False))
        cases.append((split,1,32,0,False,parts[1:]))
        cases.append((parts[1],1,32,0,False))
        mismatches = []
        observations = 0
        for case_id, case in enumerate(cases):
            wim, image, flags, status, existing = case[:5]
            references = case[5] if len(case) == 6 else []
            outputs = []
            trees = []
            for label, probe in zip(["original", "native"], probes):
                target = temp / f"target-{case_id}-{label}"
                if existing:
                    target.mkdir()
                    (target / "nested").mkdir()
                    (target / "alias").write_bytes(b"replace this")
                    os.symlink("/outside-does-not-exist", target / "empty")
                process = subprocess.run([str(probe), str(wim), str(image), str(target), str(flags), str(status), *map(str,references)], env=env, capture_output=True, text=True, check=True)
                outputs.append(process.stdout)
                trees.append(snapshot(target, image == -1, not process.stdout.endswith("extract 0\n")))
            observations += len(outputs[0].splitlines())
            if outputs[0] != outputs[1] or trees[0] != trees[1]:
                mismatches.append({"case":case_id,"codec":wim.stem,"image":image,"flags":flags,"status":status,"existing":existing,
                                   "original":outputs[0],"native":outputs[1],"original_tree":trees[0],"native_tree":trees[1]})
        evidence = {"cases":len(cases),"observations":observations,"mismatches":mismatches,
                    "original_named_cases":named_cases,
                    "oracle_cpu_workaround":"WIMLIB_DISABLE_CPU_FEATURES=sse4.2", "header":"/tmp/wimlib/include/wimlib.h"}
        output = repo / args.output
        output.parent.mkdir(parents=True,exist_ok=True)
        output.write_text(json.dumps(evidence,indent=2)+"\n")
        print(f"{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches; {output}")
        if mismatches:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
