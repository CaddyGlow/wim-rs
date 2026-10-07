#!/usr/bin/env python3
"""Execute an unchanged-header C probe inside its disposable Windows QGA guest."""
import argparse,base64,hashlib,json,pathlib,socket,time,subprocess,tempfile,os

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket',required=True,type=pathlib.Path)
    parser.add_argument('--probe',type=pathlib.Path,default=pathlib.Path('target/windows-abi-probe/probe-windows-abi.exe'))
    parser.add_argument('--dll',type=pathlib.Path,default=pathlib.Path('target/x86_64-pc-windows-msvc/debug/wim.dll'))
    parser.add_argument('--fixture',type=pathlib.Path,default=pathlib.Path('crates/wim-format/tests/fixtures/xpress-resource.wim'))
    parser.add_argument('--native-layout',type=pathlib.Path,default=pathlib.Path('target/x86_64-pc-windows-msvc/debug/examples/abi_layout.exe'))
    parser.add_argument('--guest-directory',default=r'C:\wim-abi-20261003')
    parser.add_argument('--implementation',choices=['native','native-gnu','original'],default='native')
    parser.add_argument('--output',type=pathlib.Path,default=pathlib.Path('docs/wimlib/evidence/native-windows-abi/runtime-native.json'))
    args=parser.parse_args()
    def call(command,arguments=None):
        with socket.socket(socket.AF_UNIX) as channel:
            channel.settimeout(30);channel.connect(str(args.qga_socket));channel.sendall((json.dumps({'execute':command,'arguments':arguments or {}})+'\n').encode())
            response=json.loads(channel.makefile('rb').readline())
        if 'error' in response:raise RuntimeError(response['error'])
        return response['return']
    def execute(path,arguments):
        pid=call('guest-exec',{'path':path,'arg':arguments,'capture-output':True})['pid'];deadline=time.monotonic()+300
        while time.monotonic()<deadline:
            result=call('guest-exec-status',{'pid':pid})
            if result.get('exited'):
                return {'exit':result.get('exitcode'), 'signal':result.get('signal'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8','replace'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8','replace'),'stdout_truncated':result.get('out-truncated',False),'stderr_truncated':result.get('err-truncated',False)}
            time.sleep(.5)
        raise TimeoutError(f'guest process {pid} has not exited')
    guest_root=args.guest_directory
    system=execute(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',['-NoProfile','-NonInteractive','-Command',f'New-Item -ItemType Directory -Force "{guest_root}" | Out-Null; [Environment]::OSVersion.VersionString'])
    if system['exit']!=0:raise RuntimeError(system)
    artifacts={}
    for label,path,name in [('probe',args.probe,'probe.exe'),('native_dll',args.dll,'wim.dll'),('fixture',args.fixture,'fixture-Aé漢.wim'),('native_layout',args.native_layout,'abi-layout.exe')]:
        data=path.read_bytes();guest_path=guest_root+'\\'+name
        handle=call('guest-file-open',{'path':guest_path,'mode':'wb'})
        try:
            for offset in range(0,len(data),65536):
                chunk=data[offset:offset+65536];written=call('guest-file-write',{'handle':handle,'buf-b64':base64.b64encode(chunk).decode()})
                if written['count']!=len(chunk):raise RuntimeError('short guest file write')
        finally:call('guest-file-close',{'handle':handle})
        artifacts[label]={'sha256':hashlib.sha256(data).hexdigest(),'size':len(data),'host_path':str(path),'guest_path':guest_path}
    probe=execute(artifacts['probe']['guest_path'],[artifacts['native_dll']['guest_path'],artifacts['fixture']['guest_path'],guest_root])
    native_layout=execute(artifacts['native_layout']['guest_path'],[])
    c_layout={};missing=[]
    for line in probe['stdout'].splitlines():
        parts=line.split()
        if parts and parts[0]=='layout': c_layout[parts[1]]=int(parts[2])
        if parts and parts[0]=='export' and parts[2]=='0':missing.append(parts[1])
    rust_layout=json.loads(native_layout['stdout']) if native_layout['exit']==0 else None
    if rust_layout is not None:
        # Rust example may wrap the actual measurements in a named layout object.
        measured=rust_layout.get('layout',rust_layout)
        mismatches={name:{'C':c_layout[name],'Rust':value} for name,value in measured.items() if name in c_layout and c_layout[name]!=value}
        common=sum(name in c_layout for name in measured)
    else:mismatches=None;common=0
    written=None
    if 'write-wide 0' in probe['stdout']:
        handle=call('guest-file-open',{'path':guest_root+'\\native-Aé漢.wim','mode':'rb'})
        data=bytearray()
        try:
            while True:
                chunk=call('guest-file-read',{'handle':handle,'count':65536})
                data.extend(base64.b64decode(chunk.get('buf-b64','')))
                if chunk['eof']: break
        finally:call('guest-file-close',{'handle':handle})
        output=args.probe.parent/(args.implementation+'-windows-written.wim');output.write_bytes(data)
        environment={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
        verified=subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','verify',str(output)],env=environment,capture_output=True,text=True)
        with tempfile.TemporaryDirectory(prefix='windows-native-independent-apply-') as temp:
            applied=subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','apply',str(output),'1',temp+'/target'],env=environment,capture_output=True,text=True)
            entries=sorted(str(path.relative_to(pathlib.Path(temp))) for path in pathlib.Path(temp).rglob('*'))
        written={'sha256':hashlib.sha256(data).hexdigest(),'size':len(data),'guid':bytes(data[24:40]).hex(),'guid_nonzero':any(data[24:40]),'original_verify':{'exit':verified.returncode,'stdout':verified.stdout,'stderr':verified.stderr},'original_apply':{'exit':applied.returncode,'stdout':applied.stdout,'stderr':applied.stderr,'entries':entries}}
        if verified.returncode or applied.returncode or not written['guid_nonzero']:raise RuntimeError(written)
    result={'scope':'Actual Windows guest execution of unchanged-header C against '+args.implementation+' DLL; caller CRT must match DLL CRT for FILE ownership tests','implementation':args.implementation,'guest_system':system,'artifacts':artifacts,'C_probe':probe,'native_layout':native_layout,'common_layout_fields':common,'layout_mismatches':mismatches,'missing_exports':missing,'written_WIM':written}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'guest':system['stdout'].strip(),'probe_exit':probe['exit'],'native_layout_exit':native_layout['exit'],'common_layout_fields':common,'layout_mismatches':mismatches,'missing_exports':missing,'written_WIM':written},indent=2))
    return probe['exit']!=0 or native_layout['exit']!=0 or bool(mismatches)
if __name__=='__main__':raise SystemExit(main())
