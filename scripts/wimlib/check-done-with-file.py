#!/usr/bin/env python3
"""Original-header done-with-file lifecycle differential on disposable inputs."""
import hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
import argparse
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--post-state',action='store_true');p.add_argument('--modify-after',action='store_true');p.add_argument('--output',type=Path,default=Path('docs/wimlib/evidence/native-ffi-write/done-with-file-red.json'));a=p.parse_args()
work=Path(tempfile.mkdtemp(prefix='wim-done-file-'));clients={};sha={}
for kind,lib in [('original',Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()),('native',Path('target/debug/libwim.so'))]:
 d=work/kind;d.mkdir();shutil.copy2(lib,d/'libwim.so');(d/'libwim.so.15').symlink_to('libwim.so');sha[kind]=hashlib.sha256((d/'libwim.so').read_bytes()).hexdigest()
 clients[kind]=d/'client';subprocess.run(['cc','-I/tmp/wimlib/include',('scripts/wimlib/probe-done-file-state.c' if a.post_state else 'scripts/wimlib/probe-done-with-file.c'),'-L'+str(d),'-Wl,-rpath,'+str(d),'-lwim','-o',str(clients[kind])],check=True)
rows=[];src=work/'source';target=work/'target.wim'
for layout in ['new','empty','single','hardlink','duplicate','mixed']:
 for codec in [0,1,2,3]:
  for solid in [0,0x1000,4]:
   for stop,remove in [(-1,0),(26,0),(-1,1),(-1,2),(-1,4)]:
    observations={}
    for kind,client in clients.items():
     if src.exists():shutil.rmtree(src)
     src.mkdir();target.unlink(missing_ok=True)
     if layout not in ['new','empty']:(src/'a').write_bytes(b'0'*65537)
     if layout=='hardlink':os.link(src/'a',src/'b')
     if layout=='duplicate':(src/'b').write_bytes((src/'a').read_bytes())
     if layout=='mixed':
      (src/'b').write_bytes(b'1'*13);(src/'empty').touch();(src/'link').symlink_to('a')
     r=subprocess.run([str(client),('@NEW' if layout=='new' else str(src)),str(target),str(codec),str(0x2000|solid|(1 if layout=='new' else 0)),str(stop),str(remove)],capture_output=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}|({'MODIFY_AFTER_DONE':'1'} if a.modify_after else {}))
     reader_verify = None
     if 'result=0 errno=' in r.stdout.decode():
      verified = subprocess.run(['/tmp/wimlib-native-oracle/.libs/wimlib-imagex','verify',str(target)],capture_output=True,env=os.environ|{'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2','LD_LIBRARY_PATH':str(work/'original')})
      reader_verify = {'status':verified.returncode,'stderr':verified.stderr.decode()}
     observations[kind]={'reader_verify':reader_verify,'status':r.returncode,'stdout':r.stdout.decode(),'stderr':r.stderr.decode(),'source_files':sorted(x.name for x in src.iterdir())}
    rows.append({'layout':layout,'codec':codec,'solid':solid==0x1000,'pipable':solid==4,'stop':stop,'remove':remove,'observations':observations,'exact':observations['original']==observations['native']})
for row in rows:
    original=row['observations']['original']; native=row['observations']['native']
    row['event_lifecycle_equal'] = [line for line in original['stdout'].splitlines() if line.startswith(('capture=','event='))] == [line for line in native['stdout'].splitlines() if line.startswith(('capture=','event='))] and original['source_files'] == native['source_files'] and original['status'] == native['status'] == 0
    row['result_and_errno_equal'] = [line.split(' size=')[0] for line in original['stdout'].splitlines() if line.startswith('result=')] == [line.split(' size=')[0] for line in native['stdout'].splitlines() if line.startswith('result=')]
    if a.post_state:
        original_lines = original['stdout'].splitlines()
        native_lines = native['stdout'].splitlines()
        original_descriptors = [line for line in original_lines if line.startswith('blob=')]
        native_descriptors = [line for line in native_lines if line.startswith('blob=')]
        row['ordered_descriptor_rows_equal'] = original_descriptors == native_descriptors
        row['descriptor_values_equal'] = sorted(original_descriptors) == sorted(native_descriptors)
        prefixes = ('lookup=', 'verify=', 'verify_modified=', 'retry=')
        row['post_statuses_equal'] = [line for line in original_lines if line.startswith(prefixes)] == [line for line in native_lines if line.startswith(prefixes)]
record={'modify_after':a.modify_after,'post_state':a.post_state,'work':str(work),'sha256':sha,'cases':len(rows),'exact':sum(row['exact'] for row in rows),'event_lifecycle_equal':sum(row['event_lifecycle_equal'] for row in rows),'result_and_errno_equal':sum(row['result_and_errno_equal'] for row in rows),'rows':rows};record.update({key:sum(row[key] for row in rows) for key in ('ordered_descriptor_rows_equal','descriptor_values_equal','post_statuses_equal')} if a.post_state else {});a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(record,indent=2)+'\n');print(record['cases'],record['exact'])
