#!/usr/bin/env python3
"""Compare real overwrite events and target state using disposable source copies."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

p = argparse.ArgumentParser()
p.add_argument('--native', default='target/debug/libwim.so')
p.add_argument('--source', default='/tmp/metadata-native.wim')
p.add_argument('--output', default='docs/wimlib/evidence/native-ffi-overwrite')
a = p.parse_args()
source = Path(a.source).resolve()
original_sha = hashlib.sha256(source.read_bytes()).hexdigest()
out = Path(a.output)
out.mkdir(parents=True, exist_ok=True)
cases = []
with tempfile.TemporaryDirectory(prefix='overwrite-oracle-') as work:
    work = Path(work)
    shutil.copyfile(a.native, work / 'libwim.so')
    probes = {}
    for kind, library in [('original', Path('/tmp/wimlib-native-oracle/.libs')), ('native', work)]:
        exe = work / ('probe-' + kind)
        subprocess.run(['cc', '-I/tmp/wimlib/include', 'scripts/wimlib/probe-overwrite-api.c', '-L' + str(library), '-Wl,-rpath,' + str(library), '-lwim', '-o', str(exe)], check=True)
        probes[kind] = exe
    for mutation in ['none', 'xml', 'add', 'delete', 'codec', 'invalidxml', 'readonly']:
        for flags in [0, 1, 2, 64, 128, 32768, 32784, 256]:
            for event in [0, 12, 13, 14, 15, 17]:
                results = {}
                for kind, exe in probes.items():
                    target = work / 'target.wim'
                    shutil.copyfile(source, target)
                    run = subprocess.run([str(exe), str(target), mutation, str(flags), str(event)], capture_output=True, text=True)
                    verify = None
                    if 'overwrite 0\n' in run.stdout:
                        env = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
                        check = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex', 'verify', str(target)], capture_output=True, text=True, env=env)
                        verify = dict(status=check.returncode, stdout=check.stdout.replace(str(target), 'TARGET'), stderr=check.stderr.replace(str(target), 'TARGET'))
                    results[kind] = dict(stdout=run.stdout.replace(str(target), 'TARGET'), stderr=run.stderr.replace(str(target), 'TARGET'), status=run.returncode, verify=verify)
                cases.append(dict(mutation=mutation, flags=flags, abort_event=event, exact=results['original']['stdout'] == results['native']['stdout'], **results))
    result = dict(native_sha256=hashlib.sha256((work/'libwim.so').read_bytes()).hexdigest(), count=len(cases), exact=sum(c['exact'] for c in cases), source_inputs_preserved=original_sha == hashlib.sha256(source.read_bytes()).hexdigest(), cases=cases)
    (out/'differential.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k != 'cases'}, indent=2))
