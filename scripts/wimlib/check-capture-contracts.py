#!/usr/bin/env python3
"""Preserved original C contracts for native deferred Unix capture design."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--original',default='/tmp/wimlib-native-oracle')
    parser.add_argument('--native',default='target/debug')
    parser.add_argument('--output',default='docs/wimlib/evidence/native-ffi-capture/source-contracts.json')
    args=parser.parse_args()
    env=os.environ.copy();env['WIMLIB_DISABLE_CPU_FEATURES']='sse4.2'
    results=[]
    with tempfile.TemporaryDirectory(prefix='native-capture-contract-') as temporary:
        base=pathlib.Path(temporary)
        probe=base/'original'
        subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-api.c','-L',args.original+'/.libs','-Wl,-rpath,'+args.original+'/.libs','-lwim','-o',str(probe)],check=True)
        red=subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-api.c','-L',args.native,'-Wl,-rpath,'+str(pathlib.Path(args.native).resolve()),'-lwim','-o',str(base/'native')],capture_output=True)
        link={'exit':red.returncode,'stderr':red.stderr.decode()}
        for hardlinks in [False,True]:
            for flags in [0,4,0x10,0x14,8,0x100,0x300,0x8000,0x100000,0x20,0x40,0x60]:
                for mutation in (range(6) if flags in [0,4] else [0]):
                    source=base/f's-{hardlinks}-{flags}-{mutation}';source.mkdir()
                    (source/'data').write_bytes(b'initial data')
                    if hardlinks: os.link(source/'data',source/'alias')
                    (source/'dir').mkdir();os.symlink('data',source/'link')
                    run=subprocess.run([str(probe),str(source),str(flags),'NULL',str(base/'output.wim'),'0',str(mutation),str(source/'data')],capture_output=True,env=env)
                    results.append({'hardlinks':hardlinks,'flags':flags,'mutation':mutation,'output':run.stdout.decode(),'exit':run.returncode})
        for status in [109,110,111,209,210,211]:
            source=base/f'abort-{status}';source.mkdir();(source/'data').write_bytes(b'payload')
            before=hashlib.sha256((source/'data').read_bytes()).hexdigest()
            run=subprocess.run([str(probe),str(source),'4','NULL',str(base/'output.wim'),str(status),'0'],capture_output=True,env=env)
            results.append({'flags':4,'callback_status':status,'output':run.stdout.decode(),'exit':run.returncode,'source_preserved':before==hashlib.sha256((source/'data').read_bytes()).hexdigest()})
    output=pathlib.Path(args.output);output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({'oracle':'unchanged original header and library','cases':len(results),'native_link_red':link,'results':results},indent=2)+'\n')
    print(f'{len(results)} original contract cases; native link status {link["exit"]}; {output}')
if __name__=='__main__':main()
