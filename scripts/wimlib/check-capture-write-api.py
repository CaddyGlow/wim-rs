#!/usr/bin/env python3
"""Compare captured stream reads and real output using the unchanged C header."""
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
p.add_argument('--write-flags', default='0')
p.add_argument('--codec', default='0')
p.add_argument('--output', default='docs/wimlib/evidence/native-ffi-capture/write-differential.json')
a = p.parse_args()
results = []
env = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2', WIM_CAPTURE_WRITE_FLAGS=a.write_flags, WIM_CAPTURE_CODEC=a.codec)
with tempfile.TemporaryDirectory(prefix='capture-write-oracle-') as temporary:
    base = Path(temporary)
    shutil.copyfile(a.native, base/'libwim.so')
    probes = {}
    for label, library in [('original', Path('/tmp/wimlib-native-oracle/.libs')), ('native', base)]:
        probe = base / label
        subprocess.run(['cc', '-I/tmp/wimlib/include', 'scripts/wimlib/probe-capture-write-api.c', '-L'+str(library), '-Wl,-rpath,'+str(library), '-lwim', '-o', str(probe)], check=True)
        probes[label] = probe
    for hardlinks in [False, True]:
        for flags in [0,4,0x10,0x14,8,0x100,0x300,0x8000,0x100000,0x20,0x40,0x60]:
            for mutation in (range(6) if flags in [0,4] else [0]):
                case = dict(hardlinks=hardlinks, flags=flags, mutation=mutation)
                for label, probe in probes.items():
                    source = base/'source'
                    if source.exists(): shutil.rmtree(source)
                    source.mkdir()
                    (source/'data').write_bytes(b'initial data')
                    if hardlinks: os.link(source/'data',source/'alias')
                    (source/'dir').mkdir(); os.symlink('data',source/'link')
                    target = base/'output.wim'
                    if target.exists(): target.unlink()
                    run = subprocess.run([str(probe),str(source),str(flags),'NULL',str(target),'0',str(mutation),str(source/'data')], capture_output=True, env=env)
                    checks = None
                    if b'write 0\n' in run.stdout:
                        verify = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','verify',str(target)], capture_output=True, env=env)
                        extracted = base/'extracted'
                        if extracted.exists(): shutil.rmtree(extracted)
                        apply = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','apply',str(target),'1',str(extracted)], capture_output=True, env=env)
                        tree = []
                        if not apply.returncode:
                            for path in sorted(extracted.rglob('*')):
                                if path.is_symlink(): value = 'link:'+os.readlink(path)
                                elif path.is_file(): value = hashlib.sha256(path.read_bytes()).hexdigest()
                                else: value = 'directory'
                                tree.append((str(path.relative_to(extracted)),value))
                        checks = dict(verify_status=verify.returncode,apply_status=apply.returncode,tree=tree,verify_stderr=verify.stderr.decode(errors='replace'))
                    case[label] = dict(status=run.returncode,output=run.stdout.decode(errors='replace'),checks=checks)
                case['exact'] = case['original']['output'] == case['native']['output'] and case['original']['status'] == case['native']['status']
                case['equal_apply'] = (case['original']['checks'] or {}).get('tree') == (case['native']['checks'] or {}).get('tree')
                results.append(case)
    document = dict(write_flags=a.write_flags, codec=a.codec, native_sha256=hashlib.sha256((base/'libwim.so').read_bytes()).hexdigest(),cases=len(results),exact=sum(c['exact'] for c in results),results=results)
    output=Path(a.output); output.parent.mkdir(parents=True,exist_ok=True); output.write_text(json.dumps(document,indent=2)+'\n')
    print(json.dumps({k:v for k,v in document.items() if k!='results'},indent=2))
