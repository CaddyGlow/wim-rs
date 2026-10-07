#!/usr/bin/env python3
"""Compare global lifecycle, borrowed FILE ownership, and real error messages."""
import argparse,json,subprocess,tempfile,shutil,hashlib
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--native',type=Path,required=True)
p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'))
a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='wim-diagnostics-') as d:
 t=Path(d); n=t/'native';n.mkdir();shutil.copy2(a.native/'libwim.so',n/'libwim.so')
 binaries=[]
 for label,library in [('original',a.original),('native',n)]:
  binary=t/(label+'-probe')
  subprocess.run(['cc','-I/tmp/wimlib/include','scripts/wimlib/probe-diagnostics-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(binary)],check=True)
  binaries.append(binary)
 observations=[]
 for flags in [*range(128),-1,-2147483648]:
  results=[subprocess.run([str(b),str(flags)],capture_output=True) for b in binaries]
  outputs=[(r.returncode,r.stdout,r.stderr) for r in results]
  if outputs[0]!=outputs[1]:
   print(json.dumps({'flags':flags,'original':repr(outputs[0]),'native':repr(outputs[1])},indent=2));raise SystemExit(1)
  observations.append({'flags':flags,'sha256':hashlib.sha256(results[0].stdout+results[0].stderr).hexdigest()})
 print(json.dumps({'exports':5,'cases':len(observations),'equal':True,'observations':observations},indent=2))
