#!/usr/bin/env python3
"""Compare frozen libraries with identical deterministic input and public calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess
import tempfile
import time


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def snapshot(root):
    return {str(p.relative_to(root)): (p.stat().st_size, digest(p))
            for p in sorted(root.rglob('*')) if p.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native', default='target/release/libwim.so')
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--output', default='docs/wimlib/evidence/performance/results.json')
    parser.add_argument('--cpu', type=int)
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error('repetitions must be positive')
    available = os.sched_getaffinity(0)
    cpu = args.cpu if args.cpu is not None else min(available)
    if cpu not in available:
        parser.error('requested CPU is unavailable')
    os.sched_setaffinity(0, {cpu})
    work = Path(tempfile.mkdtemp(prefix='wim-performance-'))
    source = work / 'source'
    source.mkdir()
    rng = random.Random(0x57494D)
    size = 4 * 1024 * 1024
    (source / 'random.bin').write_bytes(rng.randbytes(size))
    (source / 'zeros.bin').write_bytes(bytes(size))
    text = b'Windows component manifest synthetic benchmark: version=10.0.19041; architecture=amd64; language=en-US\n'
    (source / 'manifest.txt').write_bytes((text * (size // len(text) + 1))[:size])
    pattern = rng.randbytes(4096)
    (source / 'pattern.bin').write_bytes(pattern * (size // len(pattern)))
    for index in range(512):
        directory = source / f'dir-{index // 32:02d}'
        directory.mkdir(exist_ok=True)
        data = rng.randbytes(4096) if index % 2 else (text * 42)[:4096]
        (directory / f'file-{index:04d}.bin').write_bytes(data)
    expected = snapshot(source)
    env = {**os.environ, 'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
    libraries = {}
    callers = {}
    for label, path in [('original', Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),
                        ('native', Path(args.native).resolve())]:
        directory = work / label
        directory.mkdir()
        frozen = directory / 'libwim.so'
        shutil.copy2(path, frozen)
        (directory / 'libwim.so.15').symlink_to('libwim.so')
        caller = directory / 'probe'
        command = ['cc', '-O2', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                   'scripts/wimlib/probe-performance.c', '-L' + str(directory),
                   '-Wl,-rpath,' + str(directory), '-lwim', '-o', str(caller)]
        subprocess.run(command, check=True)
        callers[label] = caller
        libraries[label] = {'source': str(path), 'sha256': digest(frozen),
                            'caller_sha256': digest(caller), 'compile_command': command}
    modes = [('none', 0, 0), ('xpress', 1, 0), ('lzx', 2, 0),
             ('lzms', 3, 0), ('solid-lzms', 2, 1)]
    rows = []
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    record = {'work': str(work), 'cpu': cpu, 'machine': platform.platform(),
              'compilers': {name: subprocess.run([name, '--version'], capture_output=True,
                             text=True, check=True).stdout.splitlines()[0] for name in ['cc', 'rustc']},
              'original_cflags': next(line for line in Path('/tmp/wimlib-native-oracle/config.log').read_text().splitlines() if line.startswith('CFLAGS=')),
              'cpu_info': subprocess.run(['lscpu'], capture_output=True, text=True, check=True).stdout,
              'loadavg_start': Path('/proc/loadavg').read_text().strip(),
              'libraries': libraries, 'dataset': expected,
              'input_bytes': sum(v[0] for v in expected.values()),
              'repetitions': args.repetitions, 'threads': 1,
              'ordinary_chunk_bytes': 32768, 'solid_chunk_bytes': 1048576,
              'memory_measurement': '/proc/self/status VmHWM; ru_maxrss retained separately because it can include pre-exec launcher memory',
              'cpu_feature_override': 'sse4.2', 'rows': rows, 'status': 'running',
              'scope': 'Synthetic mixed files; warm page cache; one untimed warmup per mode/library; same original-written read input; default codec levels, whose native tuning is incomplete; no fsync/durable-media throughput claim.'}

    def save():
        output.write_text(json.dumps(record, indent=2) + '\n')

    def run(label, action, src, dst, codec, solid):
        result = subprocess.run([str(callers[label]), action, str(src), str(dst),
                                 str(codec), str(solid)],
                                env={**env, 'LD_LIBRARY_PATH': str(callers[label].parent)}, capture_output=True,
                                text=True, timeout=600)
        if result.returncode:
            raise RuntimeError(f'{label} {action}: {result.returncode}: {result.stderr}')
        return json.loads(result.stdout)

    save()
    for mode, codec, solid in modes:
        print(f'Benchmarking {mode}', flush=True)
        fixture = work / f'{mode}-reference.wim'
        run('original', 'write', source, fixture, codec, solid)
        for repetition in range(-1, args.repetitions):
            labels = ['original', 'native'] if repetition % 2 == 0 else ['native', 'original']
            for label in labels:
                archive = work / f'{mode}-{label}-{repetition}.wim'
                tree = work / f'{mode}-{label}-{repetition}-tree'
                written = run(label, 'write', source, archive, codec, solid)
                read = run(label, 'read', fixture, tree, codec, solid)
                if snapshot(tree) != expected:
                    raise RuntimeError(f'{mode}/{label}: extracted content differs')
                verified = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex',
                                           'verify', str(archive)], env=env,
                                          capture_output=True, text=True, timeout=600)
                if verified.returncode:
                    raise RuntimeError(f'{mode}/{label}: original verify: {verified.stderr}')
                # Check written content independently, outside all measured intervals.
                own_tree = work / f'{mode}-{label}-{repetition}-own-tree'
                run('original', 'read', archive, own_tree, codec, solid)
                if snapshot(own_tree) != expected:
                    raise RuntimeError(f'{mode}/{label}: written content differs')
                if repetition >= 0:
                    rows.append({'mode': mode, 'library': label, 'repetition': repetition,
                                 'write': written, 'read': read,
                                 'archive_bytes': archive.stat().st_size,
                                 'archive_sha256': digest(archive), 'correctness': 'passed'})
                    save()
                # Only generated benchmark output is removed; frozen libs/fixtures remain.
                shutil.rmtree(tree)
                shutil.rmtree(own_tree)
                archive.unlink()
    summary = []
    for mode, _, _ in modes:
        item = {'mode': mode}
        for label in callers:
            group = [r for r in rows if r['mode'] == mode and r['library'] == label]
            metrics = {
                'capture_write_s': [r['write']['capture_s'] + r['write']['write_s'] for r in group],
                'open_s': [r['read']['open_s'] for r in group],
                'verify_s': [r['read']['verify_s'] for r in group],
                'apply_s': [r['read']['apply_s'] for r in group],
                'write_peak_rss_mib': [r['write']['peak_rss_kib'] / 1024 for r in group],
                'read_peak_rss_mib': [r['read']['peak_rss_kib'] / 1024 for r in group],
                'archive_mib': [r['archive_bytes'] / 1048576 for r in group],
            }
            item[label] = {key: {'median': statistics.median(values), 'min': min(values),
                                'max': max(values)} for key, values in metrics.items()}
        summary.append(item)
    if snapshot(source) != expected:
        raise RuntimeError('source content changed during benchmark')
    record['source_content_preserved'] = True
    record['status'] = 'completed'
    record['summary'] = summary
    record['loadavg_end'] = Path('/proc/loadavg').read_text().strip()
    record['completed_at_unix'] = time.time()
    save()
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
