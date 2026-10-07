#!/usr/bin/env python3
"""Original hidden-generator config parsing occurs before real scan callbacks."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--native',type=Path,default=Path('target/debug/libwim.so'));p.add_argument('--output',type=Path,default=Path('docs/wimlib/evidence/native-full-upstream/fuzz/generator-config.json'));a=p.parse_args()
work=Path(tempfile.mkdtemp(prefix='wim-generator-config-'));clients={};sha={}
for kind,lib in [('original',Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()),('native',a.native)]:
 folder=work/kind;folder.mkdir();shutil.copy2(lib,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');sha[kind]=hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest();client=folder/'probe'
 subprocess.run(['cc','-I/tmp/wimlib-native-oracle','-I/tmp/wimlib/include','scripts/wimlib/probe-generator-api.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True);clients[kind]=client
configs={'valid':b'[ExclusionList]\n/file\n','exclude_all':b'[ExclusionList]\n*\n','malformed':b'bad config\n','invalid_utf8':b'\xff\n','utf16':b'\xff\xfe'+'[ExclusionList]\n*\n'.encode('utf-16le'),'unknown_section':b'[Mystery]\n*\n','missing':None}
rows=[]
for seed in [0,1,2]:
 for label,data in configs.items():
  path=work/(label+'.ini')
  if data is not None:path.write_bytes(data)
  for stop in [-1,9,11]:
   observed={}
   for kind,client in clients.items():
    target=work/'target.wim';target.unlink(missing_ok=True)
    result=subprocess.run([str(client),str(seed),str(target),str(stop),'0',str(path)],capture_output=True,text=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'})
    observed[kind]={'status':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'exists':target.exists()}
   rows.append({'seed':seed,'config':label,'stop':stop,'observations':observed,'exact':observed['original']==observed['native']})
record={'work':str(work),'sha256':sha,'cases':len(rows),'exact':sum(row['exact'] for row in rows),'rows':rows}
a.output.write_text(json.dumps(record,indent=2)+'\n');print(record['cases'],record['exact'],sha)
for row in rows:
 if not row['exact']:print(row);break
