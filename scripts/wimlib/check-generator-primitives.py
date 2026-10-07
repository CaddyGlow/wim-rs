#!/usr/bin/env python3
"""Record bounded generator goldens from unchanged original test_support.c."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
work = Path(tempfile.mkdtemp(prefix='wim-generator-primitives-'))
client = work / 'original'
command = ['cc', '-DHAVE_CONFIG_H', '-I/tmp/wimlib-native-oracle', '-I/tmp/wimlib/include', '-ffunction-sections', '-fdata-sections', 'scripts/wimlib/probe-generator-primitives.c', '/tmp/wimlib-native-oracle/.libs/libwim.a', '-Wl,--gc-sections', '-lm', '-o', str(client)]
subprocess.run(command, check=True)
rows = []
for seed in [*range(16), 0xffffffffffffffff]:
    for kind, size in [('filename',63),('filename',255),('short',0),('timestamp',0),('security',0),*[('data',size) for size in (0,1,19,257,4096)]]:
        result = subprocess.run([str(client),str(seed),kind,str(size)],capture_output=True,check=True,text=True)
        encoded, following = result.stdout.splitlines()
        payload = bytes.fromhex(encoded)
        rows.append(f'{seed}\t{kind}\t{size}\t{len(payload)}\t{hashlib.sha1(payload).hexdigest()}\t{following.removeprefix("next=")}')
fixture = Path('crates/wim/tests/fixtures/generator-primitives.tsv')
fixture.parent.mkdir(parents=True,exist_ok=True)
fixture.write_text('\n'.join(rows)+'\n')
record = {'cases':len(rows),'command':command,'work':str(work),'original_source_sha256':hashlib.sha256(Path('/tmp/wimlib/src/test_support.c').read_bytes()).hexdigest(),'original_static_library_sha256':hashlib.sha256(Path('/tmp/wimlib-native-oracle/.libs/libwim.a').read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest()}
Path('docs/wimlib/evidence/native-full-upstream/fuzz/generator-primitives-original.json').write_text(json.dumps(record,indent=2)+'\n')
print(record)
