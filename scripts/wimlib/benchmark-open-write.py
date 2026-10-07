#!/usr/bin/env python3
"""Compare frozen before/after handles and uncompressed writing on retained input."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--native', type=Path, default=Path('target/release/libwim.so'))
    parser.add_argument('--fixture-report', type=Path, required=True)
    parser.add_argument('--cpu', type=int, default=2)
    parser.add_argument('--repetitions', type=int, default=15)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.cpu not in os.sched_getaffinity(0) or args.repetitions < 1:
        parser.error('available CPU and positive repetitions are required')
    os.sched_setaffinity(0, {args.cpu})
    spec = importlib.util.spec_from_file_location('benchmark', 'scripts/wimlib/benchmark-performance.py')
    bench = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bench)
    fixture = json.loads(args.fixture_report.read_text())
    source = Path(fixture['work']) / 'source'
    reference = Path(fixture['work']) / 'none-reference.wim'
    expected = {k: tuple(v) for k, v in fixture['dataset'].items()}
    if bench.snapshot(source) != expected:
        raise RuntimeError('retained input differs from fixture report')
    work = Path(tempfile.mkdtemp(prefix='wim-open-write-'))
    libraries = {}
    callers = {}
    for label, library in [('original', Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),
                           ('before', args.before), ('after', args.native)]:
        directory = work / label
        directory.mkdir()
        frozen = directory / 'libwim.so'
        shutil.copy2(library, frozen)
        (directory / 'libwim.so.15').symlink_to('libwim.so')
        probe = directory / 'probe'
        command = ['cc', '-O2', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                   'scripts/wimlib/probe-performance.c', '-L'+str(directory),
                   '-Wl,-rpath,'+str(directory), '-lwim', '-o', str(probe)]
        subprocess.run(command, check=True)
        libraries[label] = {'source': str(library.resolve()), 'sha256': bench.digest(frozen),
                            'caller_sha256': bench.digest(probe), 'compile_command': command}
        callers[label] = probe
    rows = []
    record = {'work': str(work), 'cpu': args.cpu, 'repetitions': args.repetitions,
              'libraries': libraries, 'source': str(source), 'dataset': fixture['dataset'],
              'reference': str(reference), 'reference_sha256': bench.digest(reference),
              'loadavg_start': Path('/proc/loadavg').read_text().strip(), 'rows': rows,
              'scope': '18 MiB synthetic input; warm cache; single worker; one discarded warmup; order alternates; no fsync; verify/apply/content checks outside timing',
              'status': 'running'}
    env = {**os.environ, 'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        args.output.write_text(json.dumps(record, indent=2)+'\n')
    def run(label, action, src, dst):
        result = subprocess.run([str(callers[label]), action, str(src), str(dst), '0', '0'],
                                env=env, capture_output=True, text=True, check=True, timeout=600)
        return json.loads(result.stdout)
    save()
    for repetition in range(-1, args.repetitions):
        labels = list(callers)
        if repetition % 2:
            labels.reverse()
        for label in labels:
            archive = work / f'{label}-{repetition}.wim'
            tree = work / f'{label}-{repetition}-tree'
            own = work / f'{label}-{repetition}-own-tree'
            written = run(label, 'write', source, archive)
            read = run(label, 'read', reference, tree)
            if bench.snapshot(tree) != expected:
                raise RuntimeError('reference extraction differs')
            subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex', 'verify', str(archive)],
                           env=env, capture_output=True, check=True, timeout=600)
            run('original', 'read', archive, own)
            if bench.snapshot(own) != expected:
                raise RuntimeError('written archive differs')
            if repetition >= 0:
                rows.append({'label': label, 'repetition': repetition, 'write': written,
                             'read': read, 'archive_bytes': archive.stat().st_size, 'verified': True})
                save()
            shutil.rmtree(tree)
            shutil.rmtree(own)
            archive.unlink()
    summary = {}
    for label in callers:
        selected = [row for row in rows if row['label'] == label]
        metrics = {'capture_write_ms': [(r['write']['capture_s']+r['write']['write_s'])*1000 for r in selected],
                   'open_ms': [r['read']['open_s']*1000 for r in selected],
                   'verify_ms': [r['read']['verify_s']*1000 for r in selected],
                   'apply_ms': [r['read']['apply_s']*1000 for r in selected],
                   'write_peak_mib': [r['write']['peak_rss_kib']/1024 for r in selected],
                   'read_peak_mib': [r['read']['peak_rss_kib']/1024 for r in selected]}
        summary[label] = {k: {'median': statistics.median(v), 'min': min(v), 'max': max(v)} for k,v in metrics.items()}
    if bench.snapshot(source) != expected:
        raise RuntimeError('source changed')
    record.update(status='completed', source_preserved=True, summary=summary,
                  loadavg_end=Path('/proc/loadavg').read_text().strip())
    save()
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
