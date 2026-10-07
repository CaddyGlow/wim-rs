#!/usr/bin/env python3
"""Compare complete staged native graphs/payloads with actual original generators."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--seeds',type=int,default=32)
p.add_argument('--output',type=Path,default=Path('docs/wimlib/evidence/native-full-upstream/fuzz/generator-graphs.json'))
a=p.parse_args()
work=Path(tempfile.mkdtemp(prefix='wim-generator-graphs-'))
original=Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()
shutil.copy2(original,work/'libwim.so')
(work/'libwim.so.15').symlink_to('libwim.so')
client=work/'generate'
subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-test-generate.c','-L'+str(work),'-Wl,-rpath,'+str(work),'-lwim','-o',str(client)],check=True)
rows=[]
for seed in range(a.seeds):
    target=work/f'seed{seed}.wim'
    result=subprocess.run([str(client),str(seed),str(target)],capture_output=True,text=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'})
    rows.append({'seed':seed,'status':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'size':target.stat().st_size if target.exists() else None,'sha256':hashlib.sha256(target.read_bytes()).hexdigest() if target.exists() else None})
    if result.returncode:break
command=['cargo','test','--manifest-path','Cargo.toml','--target-dir','target','-p','wim','--all-features','--locked','--lib','generated_graphs_and_payloads','--','--ignored','--nocapture']
result=subprocess.run(command,capture_output=True,text=True,env=os.environ|{'WIM_GENERATOR_ORIGINAL_DIR':str(work),'WIM_GENERATOR_SEEDS':str(a.seeds)})
record={'scope':'staged Rust graph and every payload; public GEN flag not yet wired','work':str(work),'original_sha256':hashlib.sha256((work/'libwim.so').read_bytes()).hexdigest(),'requested_seeds':a.seeds,'original_rows':rows,'native_command':command,'native_test_status':result.returncode,'native_stdout':result.stdout,'native_stderr':result.stderr}
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps(record,indent=2)+'\n')
print(result.stdout,result.stderr)
raise SystemExit(result.returncode)
