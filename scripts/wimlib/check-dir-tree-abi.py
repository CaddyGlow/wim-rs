#!/usr/bin/env python3
"""Compare unchanged-header directory clients and independently check Rust layouts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source',type=Path,default=Path('/tmp/wimlib'))
p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'))
p.add_argument('--native',type=Path,required=True)
a=p.parse_args()
env=dict(os.environ,LD_LIBRARY_PATH=str(a.original.resolve()),WIMLIB_DISABLE_CPU_FEATURES="sse4.2")
cli=a.original/'wimlib-imagex'
with tempfile.TemporaryDirectory(prefix='wim-dir-tree-') as temporary:
    tmp=Path(temporary)
    binaries=[]
    for name,library in [('original',a.original),('native',a.native)]:
        binary=tmp/name
        subprocess.run(['cc','-I'+str((a.source/'include').resolve()),'scripts/wimlib/probe-dir-tree-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(binary)],check=True)
        binaries.append(binary)
    c_layout=subprocess.check_output([str(binaries[0]),'--layout'])
    rust_layout=subprocess.check_output([str(a.native/'examples/dir_layout')])
    if c_layout!=rust_layout: raise SystemExit('C/Rust callback layouts differ')
    tree=tmp/'tree';tree.mkdir();(tree/'dir').mkdir();(tree/'empty').mkdir()
    (tree/'alpha').write_bytes(b'payload\n'*10000)
    (tree/'dir'/'child').write_bytes(b'child\n')
    (tree/'日本語').write_bytes(b'unicode\n')
    os.link(tree/'alpha',tree/'hardlink')
    os.symlink('alpha',tree/'symlink')
    os.chmod(tree/'alpha',0o640)
    observations=[]
    for compression in ['none','XPRESS','LZX','LZMS']:
        fixture=tmp/(compression+'.wim')
        subprocess.run([str(cli),'capture',str(tree),str(fixture),'First','--compress='+compression,'--unix-data','--threads=1'],check=True,stdout=subprocess.DEVNULL,env=env)
        subprocess.run([str(cli),'append',str(tree),str(fixture),'Second','--unix-data','--threads=1'],check=True,stdout=subprocess.DEVNULL,env=env)
        cases=[(path,flags,image,0) for path in ['@NULL','','/','\\','dir','//dir\\','/dir/child','alpha','symlink','日本語','missing','/DIR','/dir/../alpha','/alpha/child','@WTF8','@BADUTF8'] for flags in [0,1,2,3,4,5,6,7,8] for image in [1,-1]]
        cases.extend([('/',1,image,0) for image in [-2,0,2,3]])
        cases.extend([('/',1,1,stop) for stop in [1,2,3,8]])
        for path,flags,image,stop in cases:
            outputs=[subprocess.check_output([str(binary),str(fixture),path,str(flags),str(image),str(stop)]) for binary in binaries]
            if outputs[0]!=outputs[1]:
                print(json.dumps({'compression':compression,'path':path,'flags':flags,'image':image,'stop':stop,'original':outputs[0].decode(),'native':outputs[1].decode()},indent=2))
                raise SystemExit('directory ABI mismatch')
            observations.append(hashlib.sha256(outputs[0]).hexdigest())
    subprocess.run([str(cli),'split',str(tmp/'none.wim'),str(tmp/'part.swm'),'0.02'],check=True,stdout=subprocess.DEVNULL,env=env)
    split_parts=sorted(tmp.glob('part*.swm'))
    for fixture in split_parts:
        for path in ['/', '/alpha', '/missing', '@BADUTF8']:
            for flags in [0,1,4,5,6,7,8]:
                for image in [1,-1,0,3]:
                    outputs=[subprocess.check_output([str(binary),str(fixture),path,str(flags),str(image),'0']) for binary in binaries]
                    if outputs[0]!=outputs[1]:
                        print(json.dumps({'fixture':fixture.name,'path':path,'flags':flags,'image':image,'original':outputs[0].decode(),'native':outputs[1].decode()},indent=2))
                        raise SystemExit('split directory ABI mismatch')
                    observations.append(hashlib.sha256(outputs[0]).hexdigest())
    subprocess.run([str(a.native/'examples/dir_fixture'),str(tmp)],check=True)
    for fixture_name in ['rich.wim','missing.wim','invalid-utf16.wim']:
        fixture=tmp/fixture_name
        for path in ['/', '/ads', '/alias', '/encrypted', '/object', '/reparse', '/missing','@WTF8','@BADUTF8']:
            for flags in range(8):
                for stop in [0,1,3]:
                    outputs=[subprocess.check_output([str(binary),str(fixture),path,str(flags),'1',str(stop)]) for binary in binaries]
                    if outputs[0]!=outputs[1]:
                        print(json.dumps({'fixture':fixture_name,'path':path,'flags':flags,'stop':stop,'original':outputs[0].decode(),'native':outputs[1].decode()},indent=2))
                        raise SystemExit('opaque directory ABI mismatch')
                    observations.append(hashlib.sha256(outputs[0]).hexdigest())
    print(json.dumps({'exports':1,'cases':len(observations),'c_rust_layout_equal':True,'layout':c_layout.decode().splitlines(),'observations_sha256':hashlib.sha256(''.join(observations).encode()).hexdigest(),'equal':True,'original_cpu_features_disabled':'sse4.2','split_parts':len(split_parts)},indent=2))
