#!/usr/bin/env python3
"""Fresh-process automatic initialization ordering through public C APIs."""
import argparse,json,subprocess,tempfile,shutil,hashlib
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--native',type=Path,required=True);p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'));a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='wim-auto-init-') as d:
 t=Path(d);n=t/'native-library';n.mkdir();shutil.copy2(a.native/'libwim.so',n/'libwim.so');binaries=[]
 for label,library in [('original',a.original),('native',n)]:
  binary=t/label;subprocess.run(['cc','-I/tmp/wimlib/include','scripts/wimlib/probe-auto-init-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(binary)],check=True);binaries.append(binary)
 observations=[];mismatches=[]
 for kind in range(3):
  for variant in range(3 if kind==0 else 5):
   for follow in [0,16,32,48,64,-1]:
    results=[subprocess.run([str(b),str(kind),str(variant),str(follow)],capture_output=True) for b in binaries];outputs=[(r.returncode,r.stdout,r.stderr) for r in results]
    case={'kind':kind,'variant':variant,'follow_flags':follow}
    if outputs[0]!=outputs[1]:mismatches.append(dict(case,original=repr(outputs[0]),native=repr(outputs[1])))
    observations.append(dict(case,sha256=hashlib.sha256(results[0].stdout+results[0].stderr).hexdigest()))
 print(json.dumps({'cases':len(observations),'equal':not mismatches,'mismatches':mismatches,'observations':observations},indent=2));raise SystemExit(bool(mismatches))
