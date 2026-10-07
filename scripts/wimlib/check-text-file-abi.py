#!/usr/bin/env python3
"""Original/native text loading, with C ownership and stdin FILE buffering."""
import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path
import subprocess
import tempfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source',type=Path,default=Path('/tmp/wimlib'))
p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'))
p.add_argument('--native',type=Path,required=True)
a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='wim-text-file-') as temporary:
    tmp=Path(temporary)
    # Freeze the cdylib: parallel Cargo links can briefly remove the original path.
    native_library=tmp/'native-library';native_library.mkdir()
    shutil.copy2(a.native/'libwim.so',native_library/'libwim.so')
    binaries=[]
    for name,library in [('original',a.original),('native',native_library)]:
        binary=tmp/name
        subprocess.run(['cc','-I'+str((a.source/'include').resolve()),'scripts/wimlib/probe-text-file-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(binary)],check=True)
        binaries.append(binary)
    fault_library=tmp/'faults.so'
    subprocess.run(['cc','-shared','-fPIC','scripts/wimlib/text-file-io-oracle.c','-ldl','-o',str(fault_library)],check=True)
    observations=[]
    def check(label,path,body=b'',mode=None,fault=False):
        arguments=[path]+([mode] if mode else [])
        env=dict(os.environ,LD_PRELOAD=str(fault_library)) if fault else None
        outputs=[subprocess.check_output([str(binary),*arguments],input=body,env=env) for binary in binaries]
        if outputs[0]!=outputs[1]:
            print(json.dumps({'case':label,'original':outputs[0].decode(),'native':outputs[1].decode()},indent=2))
            raise SystemExit('text loader ABI mismatch')
        observations.append({'case':label,'sha256':hashlib.sha256(outputs[0]).hexdigest(),'result':int(outputs[0].splitlines()[0].split(b'=')[1])})
    payloads=[b'',b'a',b'hello\r\nworld\rfinal\n',b'\xef\xbb\xbf',b'\xef\xbb\xbfhello',b'\xff\xfe',b'\xff\xfeA\0B\0',b'A\0B\0',b'A\0B',b'\xff\xfeA',b'\xff\xfe\0\xd8',b'\xff\xfe\0\xdc',b'\xff\xfe\0\xd8\0\xdc',b'\xff\xfe\xfe\xff\xff\xff',b'\xfe\xffA\0B\0',b'\xff\xfe\0\0A\0\0\0B\0',b'hello\0world',b'\xff\xc0\x80\xe0\x80\x80',b'\xed\xa0\x80',b'\xef\xbb\xbf\xff',b'\xff\xfe\x80',b'\x80\0A\0',b'\0\0',b'\0A',bytes(range(256)),b'X'*255,b'X'*256,b'X'*257,b'X'*768,b'X'*769,b'X'*(1024*1024)]
    payloads.append(b'\xff\xfe'+b''.join(unit.to_bytes(2,'little') for unit in range(65536)))
    for byte in range(256):payloads.append(bytes([byte]))
    for unit in range(0xd800,0xe000):payloads.append(b'\xff\xfe'+unit.to_bytes(2,'little'))
    for i,payload in enumerate(payloads):
        fixture=tmp/'payload.txt';fixture.write_bytes(payload)
        check('file-'+str(i),str(fixture))
        # Include both stdin selectors and C buffered pushback at representative boundaries.
        if i<31:
            check('stdin-null-'+str(i),'@NULL',payload)
            check('stdin-dash-'+str(i),'-',payload)
            check('stdin-pushback-'+str(i),'@NULL',payload,'pushback')
    denied=tmp/'unreadable';denied.write_bytes(b'unreadable');denied.chmod(0)
    try:check('permission-denied',str(denied))
    finally:denied.chmod(0o600)
    dash=tmp/'-';dash.write_bytes(b'literal dash filename')
    check('literal-dash-filename',str(dash))
    for fault in ['forced-eof.txt','forced-read-error.txt','forced-stat-error.txt']:
        path=tmp/fault;path.write_bytes(b'advertised file contents')
        check(fault,str(path),fault=True)
    check('empty-path','')
    check('missing',str(tmp/'missing'))
    check('directory',str(tmp))
    check('stdin-closed','@NULL',mode='closed')
    check('proc-advertised-zero','/proc/version')
    raw_path=os.fsencode(tmp)+b'/raw-\xff-name';fd=os.open(raw_path,os.O_WRONLY|os.O_CREAT,0o600);os.write(fd,b'raw filename');os.close(fd)
    check('raw-byte-filename',raw_path)
    print(json.dumps({'exports':1,'native_library_sha256':hashlib.sha256((native_library/'libwim.so').read_bytes()).hexdigest(),'cases':len(observations),'equal':True,'observations_sha256':hashlib.sha256(json.dumps(observations,sort_keys=True).encode()).hexdigest(),'cases_by_result':{str(result):sum(o['result']==result for o in observations) for result in sorted({o['result'] for o in observations})}},indent=2))
