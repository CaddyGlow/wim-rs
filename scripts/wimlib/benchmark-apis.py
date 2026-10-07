#!/usr/bin/env python3
"""Benchmark matched safe Rust and C workloads in the same release executable."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import statistics
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CODECS = {"none": 0, "xpress": 1, "lzx": 2, "lzms": 3}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def snapshot(root):
    return {str(p.relative_to(root)): digest(p) for p in sorted(root.rglob("*")) if p.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, help="existing release benchmark_apis executable")
    parser.add_argument("--source", type=Path, help="existing input directory; never modified")
    parser.add_argument("--size-mib", type=int, default=4, help="size of each generated large file")
    parser.add_argument("--small-files", type=int, default=512)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--codecs", nargs="+", choices=CODECS, default=list(CODECS))
    parser.add_argument("--oracle", type=Path, help="optional original wimlib-imagex verifier")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.repetitions < 1 or args.size_mib < 1 or args.small_files < 0:
        parser.error("repetitions and size must be positive; small-files must be nonnegative")
    if args.probe is None:
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "wim", "--example", "benchmark_apis"], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
        args.probe = Path(metadata["target_directory"]) / "release/examples/benchmark_apis"
    work = Path(tempfile.mkdtemp(prefix="wim-api-benchmark-"))
    probe = work / "benchmark_apis"
    shutil.copy2(args.probe, probe)
    source = args.source.resolve() if args.source else work / "source"
    if not args.source:
        source.mkdir()
        size = args.size_mib * 1024 * 1024
        rng = random.Random(42)
        (source / "random").write_bytes(rng.randbytes(size))
        (source / "zeros").write_bytes(bytes(size))
        for name, pattern in [("text", b"Windows image benchmark manifest\n"), ("pattern", bytes(range(256)))]:
            (source / name).write_bytes((pattern * (size // len(pattern) + 1))[:size])
        small = source / "small"
        small.mkdir()
        for i in range(args.small_files):
            (small / str(i)).write_bytes(rng.randbytes(4096))
    expected = snapshot(source)
    report = {"status": "running", "work_directory": str(work), "probe_sha256": digest(probe),
              "cargo_lock_sha256": digest(ROOT / "Cargo.lock"), "source": str(source),
              "source_files": expected, "repetitions": args.repetitions, "warmups": 1,
              "affinity": sorted(os.sched_getaffinity(0)), "oracle": str(args.oracle) if args.oracle else None,
              "options": "default capture/open/extract; all images; integrity; default chunks and threads; no solid",
              "samples": [], "summary": {}}
    args.output.parent.mkdir(parents=True, exist_ok=True)

    def save():
        args.output.write_text(json.dumps(report, indent=2) + "\n")

    def run(api, action, src, dst, codec):
        return json.loads(subprocess.check_output([str(probe), api, action, str(src), str(dst), str(codec)], timeout=600))

    try:
        save()
        for name in args.codecs:
            codec = CODECS[name]
            reference = work / f"reference-{name}.wim"
            run("c", "write", source, reference, codec)
            for repetition in range(-1, args.repetitions):
                order = ["rust", "c"] if repetition % 2 == 0 else ["c", "rust"]
                for api in order:
                    prefix = work / f"{name}-{repetition}-{api}"
                    archive = prefix.with_suffix(".wim")
                    extracted = Path(str(prefix) + "-read")
                    written = run(api, "write", source, archive, codec)
                    read = run(api, "read", reference, extracted, codec)
                    if snapshot(extracted) != expected:
                        raise RuntimeError(f"{api} {name}: extracted reference differs from source")
                    cross = Path(str(prefix) + "-cross")
                    run("c" if api == "rust" else "rust", "read", archive, cross, codec)
                    if snapshot(cross) != expected:
                        raise RuntimeError(f"{api} {name}: written archive differs from source")
                    if args.oracle:
                        subprocess.run([str(args.oracle.resolve()), "verify", str(archive)], check=True, stdout=subprocess.DEVNULL, timeout=600)
                    if repetition >= 0:
                        report["samples"].append({"api": api, "codec": name, "repetition": repetition,
                            "capture_s": written["capture_s"], "write_s": written["write_s"],
                            "capture_write_s": written["capture_s"] + written["write_s"],
                            "open_s": read["open_s"], "verify_s": read["verify_s"], "apply_s": read["apply_s"],
                            "write_peak_rss_kib": written["peak_rss_kib"], "read_peak_rss_kib": read["peak_rss_kib"],
                            "archive_bytes": archive.stat().st_size})
                    shutil.rmtree(extracted)
                    shutil.rmtree(cross)
                    archive.unlink()
                print(f"{name}: {'warmup' if repetition < 0 else f'repetition {repetition + 1}'} complete", flush=True)
                save()
        if snapshot(source) != expected:
            raise RuntimeError("input changed during benchmark")
        for name in args.codecs:
            report["summary"][name] = {}
            for api in ["rust", "c"]:
                samples = [s for s in report["samples"] if s["codec"] == name and s["api"] == api]
                report["summary"][name][api] = {key: {"median": statistics.median(s[key] for s in samples),
                    "min": min(s[key] for s in samples), "max": max(s[key] for s in samples)}
                    for key in samples[0] if key not in {"api", "codec", "repetition"}}
        report["status"] = "complete"
        save()
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = str(error)
        save()
        raise
    print(f"Results: {args.output}")


if __name__ == "__main__":
    main()
