#!/usr/bin/env python3
"""Probe genuine original test-support image comparisons against native."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

out = Path('docs/wimlib/evidence/native-full-upstream/fuzz')
out.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='wim-test-support-'))
clients = {}
sha = {}
for kind, lib in [('original',Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()),('native',Path('target/debug/libwim.so'))]:
    folder = work/kind
    folder.mkdir()
    shutil.copy2(lib,folder/'libwim.so')
    (folder/'libwim.so.15').symlink_to('libwim.so')
    sha[kind] = hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest()
    client = folder/'probe'
    subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-compare-images.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True)
    clients[kind] = client
fixtures = Path('crates/wim-format/tests/fixtures')
files = ['pipe-none.wim','pipable-resource.wim','pipe-lzx.wim','pipe-lzms.wim','pipe-two-image.wim','pipe-xpress-64k.wim','solid-resource.wim']
generator = work/'original'/'generate'
subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-test-generate.c','-L'+str(work/'original'),'-Wl,-rpath,'+str(work/'original'),'-lwim','-o',str(generator)],check=True)
for seed in range(8):
    generated=work/f'seed{seed}.wim'
    subprocess.run([str(generator),str(seed),str(generated)],check=True,capture_output=True)
    files.append(str(generated))
rows=[]
for a in files:
    for b in files:
        for flag in [0,1,2,4,8,9,15,-1,16]:
            case=[str((fixtures/a).resolve()),'1',str((fixtures/b).resolve()),'1',str(flag)]
            observed={}
            for kind,client in clients.items():
                result=subprocess.run([str(client),*case],capture_output=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'})
                observed[kind]={'status':result.returncode,'stdout':result.stdout.decode(),'stderr':result.stderr.decode()}
            rows.append({'a':a,'b':b,'flags':flag,'observed':observed,'exact':observed['original']['status']==observed['native']['status']==0 and observed['original']['stdout']==observed['native']['stdout']})
record={'work':str(work),'sha256':sha,'cases':len(rows),'exact':sum(row['exact'] for row in rows),'rows':rows}
(out/'compare-differential.json').write_text(json.dumps(record,indent=2)+'\n')
print(record['cases'],record['exact'],sha)
