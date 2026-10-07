#!/usr/bin/env python3
"""Record unchanged-header progress comparisons without normalizing differences."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--native', type=Path, default=ROOT / 'target/debug/libwim.so')
parser.add_argument('--oracle', type=Path, default=Path('/tmp/wimlib-native-oracle'))
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
# Freeze the actual built artifact, so concurrent team builds cannot change it.
work = Path(tempfile.mkdtemp(prefix='wim-progress-oracle-'))
shutil.copy2(args.native, work / 'libwim.so')
executables = {}
for name, library in [('original', args.oracle / '.libs'), ('native', work)]:
    exe = work / name
    subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-DPROBE_JOIN_PROGRESS',
                    '-I/tmp/wimlib/include', str(ROOT / 'scripts/wimlib/probe-progress-api.c'),
                    '-L', str(library), f'-Wl,-rpath,{library}', '-lwim', '-o', str(exe)], check=True)
    executables[name] = exe
base = ROOT / 'docs/wimlib/evidence/native-ffi-progress'
records = []
for filename in ['write-red.json', 'write-modes-red.json', 'write-metadata-xml-red.json', 'write-file-state-red.json']:
    for prior in json.loads((base / filename).read_text()):
        environment = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
        if prior.get('invalid_xml'):
            environment['WIM_PROGRESS_INVALID_XML'] = '1'
        source = Path(prior['args'][0])
        source_before = hashlib.sha256(source.read_bytes()).hexdigest() if source.is_file() else None
        observations = {}
        for name, exe in executables.items():
            target = Path('/tmp/wim-progress-written.wim')
            target.write_bytes(b'previous-output-marker')
            result = subprocess.run([str(exe), *prior['args']], capture_output=True, text=True, env=environment)
            observations[name] = {'exit': result.returncode, 'stdout': result.stdout.splitlines(),
                                  'stderr': result.stderr, 'output_size': target.stat().st_size,
                                  'output_unchanged': target.read_bytes() == b'previous-output-marker'}
            if source_before is not None:
                assert hashlib.sha256(source.read_bytes()).hexdigest() == source_before
            if 'result 0' in observations[name]['stdout']:
                verify = subprocess.run([str(args.oracle / 'wimlib-imagex'), 'verify', str(target)],
                                        capture_output=True, text=True, env=environment)
                observations[name]['independent_verify'] = {'exit': verify.returncode, 'stdout': verify.stdout, 'stderr': verify.stderr}
        original, native = observations['original'], observations['native']
        records.append({'matrix': filename, 'args': prior['args'], 'invalid_xml': prior.get('invalid_xml', False),
                        **observations, 'events_equal': original['stdout'] == native['stdout'],
                        'output_size_equal': original['output_size'] == native['output_size']})
summary = {'native_artifact': str(args.native), 'frozen_artifact': str(work / 'libwim.so'),
           'native_sha256': hashlib.sha256((work / 'libwim.so').read_bytes()).hexdigest(),
           'count': len(records), 'exact_events': sum(r['events_equal'] for r in records),
           'exact_sizes': sum(r['output_size_equal'] for r in records), 'source_inputs_preserved': True,
           'cases': records}
(args.output / 'writer-current.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({k: v for k, v in summary.items() if k != 'cases'}, indent=2))
# Retain all genuine codec differences in the result; they remain partial gates.
