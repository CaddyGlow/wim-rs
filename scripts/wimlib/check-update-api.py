#!/usr/bin/env python3
"""Compare journal rollback and canonical update callbacks with original C."""
import argparse
import hashlib
import itertools
import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--native',type=Path,required=True)
p.add_argument('--source',type=Path,default=Path('/tmp/wimlib'))
p.add_argument('--oracle',type=Path,default=Path('/tmp/wimlib-native-oracle'))
a=p.parse_args()
results=[]
with tempfile.TemporaryDirectory(prefix='wim-update-') as directory:
 root=Path(directory);clients=[]
 frozen=root/'library';frozen.mkdir()
 shutil.copy2(a.native/'libwim.so',frozen/'libwim.so')
 native_sha256=hashlib.sha256((frozen/'libwim.so').read_bytes()).hexdigest()
 for name,library in [('original',a.oracle/'.libs'),('native',frozen)]:
  client=root/name
  subprocess.run(['cc','-Wall','-Wextra','-Werror','-I'+str((a.source/'include').resolve()),'scripts/wimlib/probe-update-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(client)],check=True)
  clients.append(client)
 tree=root/'tree';tree.mkdir();(tree/'dir').mkdir()
 (tree/'file').write_bytes(b'journal data\n'*100)
 os.link(tree/'file',tree/'alias');(tree/'dir'/'child').write_bytes(b'nested')
 source=root/'source.wim'
 env=dict(os.environ,WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
 subprocess.run([str(a.oracle/'wimlib-imagex'),'capture',str(tree),str(source),'Journal','--compress=none','--unix-data','--threads=1'],stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,env=env,check=True)
 for scenario,flags,image,abort,status,change in itertools.product(range(10),[0,1,2,-1],[0,1,2],range(5),[1,2,-1],[0,1,2]):
  arguments=list(map(str,[scenario,flags,abort,status,image,change]))
  outputs=[subprocess.check_output([str(client),str(source),*arguments],env=env) for client in clients]
  assert outputs[0]==outputs[1],(arguments,outputs)
  results.append({'arguments':arguments,'stdout_sha256':hashlib.sha256(outputs[0]).hexdigest(),'stdout':outputs[0].decode()})
print(json.dumps({'cases':len(results),'native_sha256':native_sha256,'scope':'DELETE/RENAME and zero-command transactions; ADD pending','results':results},indent=2))
