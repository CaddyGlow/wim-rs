#!/usr/bin/env python3
"""Test actual incremental backup reuse and source-read lifetime against original."""
import hashlib,json,os,pathlib,runpy,shutil,subprocess,tempfile

def main():
    snapshot=runpy.run_path('scripts/wimlib/check-extract-api.py')['snapshot'];env={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'};rows=[]
    with tempfile.TemporaryDirectory(prefix='template-incremental-api-') as temporary:
        base=pathlib.Path(temporary);clients=[];hashes=[]
        for label,library in [('original',pathlib.Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),('native',pathlib.Path('target/debug/libwim.so'))]:
            folder=base/label;folder.mkdir();shutil.copyfile(library,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');hashes.append(hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest());client=folder/'client';clients.append(client)
            subprocess.run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include','scripts/wimlib/probe-template-image.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True)
        cases=[(same,hardlink,remove,exclude) for same in [False,True] for hardlink in [False,True] for remove in [False,True] for exclude in ([None,"alias","file"] if hardlink else [None])]
        for index,(same,hardlink,remove,exclude) in enumerate(cases):
            pair=[]
            for label,client in zip(['original','native'],clients):
                source=base/f'{index}-{label}-source';source.mkdir();(source/'file').write_bytes(bytes(range(256))*300+b'last chunk')
                if hardlink: os.link(source/'file',source/'alias')
                for path in [source,*source.iterdir()]:os.utime(path,ns=(1600000000123456700,1600000001765432100),follow_symlinks=False)
                config_args=[]
                if exclude:
                    config=base/f'{index}-{label}-config';config.write_text('[ExclusionList]\n'+exclude+'\n');config_args=['--config='+str(config)]
                template=base/f'{index}-{label}-template.wim';subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','capture',str(source),str(template),'--compress=none','--no-acls','--nocheck','--threads=1',*config_args],env=env,capture_output=True,check=True)
                captured=snapshot(source)
                for entry in captured: entry.pop('mtime',None);entry.pop('atime',None)
                template_hash=hashlib.sha256(template.read_bytes()).hexdigest()
                output=base/f'{index}-{label}-output.wim';case_env=dict(env)
                if same:case_env['TEMPLATE_SAME_HANDLE']='1'
                if remove:case_env.update(TEMPLATE_UNLINK_FILE=str(source/'file'),TEMPLATE_UNLINK_ALIAS=str(source/'alias'))
                run=subprocess.run([str(client),str(template),str(source),'2' if same else '1','1','0','0',str(output)],env=case_env,capture_output=True,text=True,check=True)
                result={'output':run.stdout,'verify':None,'tree':None}
                if run.stdout.endswith('write 0\n'):
                    verify=subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','verify',str(output)],env=env,capture_output=True);result['verify']=verify.returncode
                    target=base/f'{index}-{label}-target';applied=subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','apply',str(output),'2' if same else '1',str(target)],env=env,capture_output=True);result['apply']=applied.returncode;result['tree']=snapshot(target)
                    # Independent apply proves payload and hardlink topology, while capture timestamps differ.
                    for entry in result['tree']: entry.pop('mtime',None);entry.pop('atime',None)
                    result['matches_capture']=result['tree']==captured
                result['template_preserved']=hashlib.sha256(template.read_bytes()).hexdigest()==template_hash
                if not result['template_preserved']: raise RuntimeError('template input changed')
                if result.get('apply')==0 and not result['matches_capture']: raise RuntimeError('applied image differs from captured source')
                pair.append(result)
            rows.append({'template_exclusion':exclude,'same_handle':same,'hardlinks':hardlink,'unlink_after_reference':remove,'original':pair[0],'native':pair[1],'exact':pair[0]==pair[1]})
    output=pathlib.Path('docs/wimlib/evidence/native-ffi-template/incremental-differential.json');output.write_text(json.dumps({'cases':len(rows),'exact':sum(row['exact'] for row in rows),'library_sha256':dict(zip(['original','native'],hashes)),'results':rows},indent=2)+'\n');print(f'{len(rows)} cases; {sum(row["exact"] for row in rows)} exact');return any(not row['exact'] for row in rows)
if __name__=='__main__':raise SystemExit(main())
