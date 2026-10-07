#!/usr/bin/env python3
"""Compare original solid resource ordering and true file lifecycle."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,default=Path('docs/wimlib/evidence/native-ffi-write/solid-sort-red.json'));a=p.parse_args()
work=Path(tempfile.mkdtemp(prefix='wim-solid-sort-'));clients={};sha={}
for kind,lib in [('original',Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()),('native',Path('target/debug/libwim.so'))]:
 folder=work/kind;folder.mkdir();shutil.copy2(lib,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');sha[kind]=hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest();client=folder/'probe'
 subprocess.run(['cc','-I/tmp/wimlib/include','scripts/wimlib/probe-solid-sort.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True);clients[kind]=client
source=work/'files';source.mkdir()
for index,name in enumerate(['a.zzz','b.aaa','c.TXT','d.txt','e','.hidden','trailing.','x\\component.dat','f.long']):(source/name).write_bytes(bytes([index+1])*(8192+index*317))
os.link(source/'f.long',source/'g.z')
rows=[];target=work/'target.wim'
for mode in [0,1,2]:
 for flags in [0,0x4000,0x1000,0x5000]:
  for codec in [1,2]:
   observed={}
   for kind,client in clients.items():
    target.unlink(missing_ok=True);Path(str(target)+'.source').unlink(missing_ok=True)
    result=subprocess.run([str(client),str(source),str(target),str(mode),str(flags),str(codec)],capture_output=True,text=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'})
    observed[kind]={'status':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'exists':target.exists()}
   rows.append({'mode':mode,'flags':flags,'codec':codec,'observations':observed,'exact':observed['original']==observed['native']})
record={'work':str(work),'sha256':sha,'cases':len(rows),'exact':sum(row['exact'] for row in rows),'rows':rows};a.output.write_text(json.dumps(record,indent=2)+'\n');print(record['cases'],record['exact'],sha)
