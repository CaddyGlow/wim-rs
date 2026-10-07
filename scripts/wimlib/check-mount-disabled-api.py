#!/usr/bin/env python3
"""Prove the explicit no-FUSE build gate against the unchanged original API."""
import hashlib,json,os,pathlib,shutil,subprocess,tempfile

def main():
    rows=[];env={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
    fixture=pathlib.Path('crates/wim-format/tests/fixtures/xpress-resource.wim').resolve()
    source_hash=hashlib.sha256(fixture.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix='mount-disabled-api-') as temporary:
        base=pathlib.Path(temporary);target=base/'target';target.mkdir();(target/'preserved').write_bytes(b'unchanged mount target')
        for label,source in [('original',pathlib.Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),('native',pathlib.Path('target/debug/libwim.so'))]:
            folder=base/label;folder.mkdir();shutil.copyfile(source,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');client=base/(label+'-client')
            built=subprocess.run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include','scripts/wimlib/probe-mount-disabled-api.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],capture_output=True,text=True)
            row={'library':label,'sha256':hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest(),'link_return':built.returncode}
            if built.returncode: row['link_error']=built.stderr
            else:
                run=subprocess.run([str(client),str(fixture),str(target)],env=env,capture_output=True,text=True);row.update(exit=run.returncode,output=run.stdout,diagnostic=run.stderr,observations=len(run.stdout.splitlines()))
            rows.append(row)
        preserved=(target/'preserved').read_bytes()==b'unchanged mount target' and sorted(p.name for p in target.iterdir())==['preserved'] and hashlib.sha256(fixture.read_bytes()).hexdigest()==source_hash
    exact=rows[1]['link_return']==0 and rows[0].get('exit')==rows[1].get('exit')==0 and rows[0].get('output')==rows[1].get('output') and rows[0].get('diagnostic')==rows[1].get('diagnostic')
    output=pathlib.Path('docs/wimlib/evidence/native-ffi-mount-disabled/differential.json');output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({'scope':'explicit no-FUSE build capability; not filesystem mount support','original_configure':'--without-fuse --without-ntfs-3g --enable-test-support','host_dev_fuse_exists':pathlib.Path('/dev/fuse').exists(),'source_sha256':source_hash,'inputs_and_target_preserved':preserved,'exact':exact,'results':rows},indent=2)+'\n')
    print(f'{rows[0].get("observations",0)} observations; exact={exact}; preserved={preserved}')
    return not(exact and preserved)
if __name__=='__main__':raise SystemExit(main())
