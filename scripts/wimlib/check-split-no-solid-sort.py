#!/usr/bin/env python3
"""Frozen unchanged-header split NO_SOLID_SORT source policy differential."""
import argparse,hashlib,json,os,pathlib,shutil,subprocess,tempfile
p=argparse.ArgumentParser();p.add_argument('--output',required=True);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];work=pathlib.Path(tempfile.mkdtemp(prefix='wim-split-no-sort-'));env={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
def run(cmd):
 r=subprocess.run(list(map(str,cmd)),env=env,capture_output=True,text=True);return dict(status=r.returncode,stdout=r.stdout,stderr=r.stderr)
sha={}
for label,source in [('original',pathlib.Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),('native',root/'target/debug/libwim.so')]:
 d=work/label;d.mkdir();shutil.copy2(source,d/'libwim.so');(d/'libwim.so.15').symlink_to('libwim.so');sha[label]=hashlib.sha256((d/'libwim.so').read_bytes()).hexdigest();r=run(['cc','-I/tmp/wimlib/include',root/'scripts/wimlib/probe-split-join-api.c','-L'+str(d),'-Wl,-rpath,'+str(d),'-lwim','-o',d/'client']);assert r['status']==0,r
files=work/'files';files.mkdir();(files/'a').write_bytes(bytes((i*17+i//33)%256 for i in range(120000)));(files/'b').write_bytes(b'hello'*17000)
cli=pathlib.Path('/tmp/wimlib-native-oracle/.libs/wimlib-imagex');env['LD_LIBRARY_PATH']=str(work/'original');rows=[]
for codec in ['none','XPRESS','LZX','LZMS']:
 for solid in [False,True]:
  source=work/(codec+str(solid)+'.wim');cmd=[cli,'capture',files,source,'--compress='+codec,'--no-acls'];
  if solid:cmd+=['--solid']
  r=run(cmd);assert r['status']==0,r
  before=hashlib.sha256(source.read_bytes()).hexdigest()
  for flag in [0,0x4000,0x1000,0x5000,4,5,0x2000,0x2001,0x10,0x11,0x2011]:
   observations={}
   for label in ['original','native']:
    dest=work/'output';shutil.rmtree(dest,ignore_errors=True);dest.mkdir();env['LD_LIBRARY_PATH']=str(work/label)
    r=run([work/label/'client','split',source,dest/'part.swm',100000,flag|0x800|(0 if flag & 4 else 8),'none']);parts=sorted(dest.glob('*.swm'));r['parts']=len(parts)
    env['LD_LIBRARY_PATH']=str(work/'original')
    r['original_verifies']=[run([cli,'verify',part])['status'] for part in parts]
    if parts:
     joined=work/'joined.wim';joined.unlink(missing_ok=True)
     r['original_join']=run([cli,'join',joined,*parts])['status']
     r['joined_verify']=run([cli,'verify',joined])['status']
     applied=work/'applied';shutil.rmtree(applied,ignore_errors=True)
     r['original_apply']=run([cli,'apply',joined,'1',applied])['status']
     r['files']={str(f.relative_to(applied)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(applied.rglob('*')) if f.is_file()}
    observations[label]=r
   rows.append(dict(codec=codec,solid=solid,flags=flag,observations=observations,exact=observations['original']==observations['native']))
  assert before==hashlib.sha256(source.read_bytes()).hexdigest()
record=dict(work=str(work),sha256=sha,cases=len(rows),exact=sum(r['exact'] for r in rows),rows=rows,input_sources_preserved=True);pathlib.Path(a.output).write_text(json.dumps(record,indent=2)+'\n');print(record['cases'],record['exact'],sha)
