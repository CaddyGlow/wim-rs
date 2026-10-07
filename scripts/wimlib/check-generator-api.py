#!/usr/bin/env python3
"""Original-header generated capture, progress, descriptors and written graph oracle."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--seeds',type=int,default=32);p.add_argument('--output',type=Path,default=Path('docs/wimlib/evidence/native-full-upstream/fuzz/generator-api-final.json'));a=p.parse_args()
work=Path(tempfile.mkdtemp(prefix='wim-generator-api-'));clients={};sha={}
for kind,lib in [('original',Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()),('native',Path('target/debug/libwim.so'))]:
 folder=work/kind;folder.mkdir();shutil.copy2(lib,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');sha[kind]=hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest()
 client=folder/'probe';subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-generator-api.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True);clients[kind]=client
compare=work/'compare';subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-compare-images.c','-L'+str(work/'original'),'-Wl,-rpath,'+str(work/'original'),'-lwim','-o',str(compare)],check=True)
rows=[];env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
for seed in range(a.seeds):
 for stop in [-1,9,11]:
  for flags in [0,4,0x1000]:
   obs={};targets={}
   for kind,client in clients.items():
    target=work/'target.wim';target.unlink(missing_ok=True);targets[kind]=work/f'{kind}-{seed}-{stop}-{flags}.wim'
    result=subprocess.run([str(client),str(seed),str(target),str(stop),str(flags)],capture_output=True,text=True,env=env)
    verify=None
    if target.exists():
     checked=subprocess.run(['/tmp/wimlib-native-oracle/.libs/wimlib-imagex','verify',str(target)],capture_output=True,text=True,env=env|{'LD_LIBRARY_PATH':str(work/'original')})
     verify={'status':checked.returncode,'stderr':checked.stderr}
    if target.exists():shutil.copy2(target,targets[kind])
    obs[kind]={'status':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'exists':target.exists(),'verify':verify}
   comparison=None
   if all(target.exists() for target in targets.values()):
    checked=subprocess.run([str(compare),str(targets['original']),'1',str(targets['native']),'1','0'],capture_output=True,text=True,env=env)
    comparison={'status':checked.returncode,'stdout':checked.stdout,'stderr':checked.stderr}
   rows.append({'seed':seed,'stop':stop,'flags':flags,'observations':obs,'exact':obs['original']==obs['native'],'original_graph_comparison':comparison})
record={'work':str(work),'sha256':sha,'cases':len(rows),'exact':sum(row['exact'] for row in rows),'rows':rows};a.output.write_text(json.dumps(record,indent=2)+'\n');print(record['cases'],record['exact'],sha)
