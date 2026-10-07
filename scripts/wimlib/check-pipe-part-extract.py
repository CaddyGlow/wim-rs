#!/usr/bin/env python3
"""Compare genuine original-created split-pipable transitions and cancellation."""
import hashlib,json,os,pathlib,runpy,shutil,subprocess,tempfile,threading

def main():
    env = {**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
    snapshot = runpy.run_path('scripts/wimlib/check-extract-api.py')['snapshot']
    mismatches=[];results=[];payload_hashes={}
    with tempfile.TemporaryDirectory(prefix='pipe-part-extract-') as temporary:
        base=pathlib.Path(temporary);source=base/'source';source.mkdir()
        for index in range(3): (source/f'data{index}').write_bytes(bytes((i*17+index*47)%256 for i in range(40003+index)))
        os.link(source/'data0',source/'alias');(source/'link').symlink_to('data2')
        for path in [source,*source.iterdir()]: os.utime(path,ns=(1600000000123456700,1600000001765432100),follow_symlinks=False)
        libraries=[];clients=[]
        for label,original in [('original',pathlib.Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),('native',pathlib.Path('target/debug/libwim.so'))]:
            folder=base/label;folder.mkdir();shutil.copyfile(original,folder/'libwim.so');(folder/'libwim.so.15').symlink_to('libwim.so');libraries.append(hashlib.sha256((folder/'libwim.so').read_bytes()).hexdigest())
            client=base/(label+'-client');subprocess.run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include','scripts/wimlib/probe-pipe-part-extract.c','-L'+str(folder),'-Wl,-rpath,'+str(folder),'-lwim','-o',str(client)],check=True);clients.append(client)
        generator=base/'generate';subprocess.run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include','scripts/wimlib/probe-generate-pipable-parts.c','-L'+str(base/'original'),'-Wl,-rpath,'+str(base/'original'),'-lwim','-o',str(generator)],check=True)
        cases=[];payloads={};part_evidence={}
        for codec in ['none','XPRESS','LZX','LZMS']:
            wim=base/(codec+'.wim');subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex','capture',str(source),str(wim),'--pipable','--compress='+codec,'--unix-data','--nocheck','--no-acls','--threads=1'],env=env,capture_output=True,check=True)
            folder=base/('parts-'+codec);folder.mkdir();subprocess.run([str(generator),str(wim),str(folder/'part.swm'),'1'],env=env,capture_output=True,check=True)
            parts=sorted(folder.glob('*.swm'),key=lambda path: int.from_bytes(path.read_bytes()[40:42],'little'))
            if len(parts)<2: raise RuntimeError('expected genuine original split parts')
            data=b''.join(path.read_bytes() for path in parts);payloads[codec]=data
            part_evidence[codec]=[{'part':int.from_bytes(path.read_bytes()[40:42],'little'),'total':int.from_bytes(path.read_bytes()[42:44],'little'),'size':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for path in parts]
            payloads[codec+'-missing-last']=b''.join(path.read_bytes() for path in parts[:-1])
            # Repeated actual header from the original first part must not emit a duplicate transition.
            first=parts[0].read_bytes();payloads[codec+'-repeat-header']=first+first[:208]+data[len(first):]
            for layout in [codec,codec+'-missing-last',codec+'-repeat-header']:
                for stop in [0,1,2,len(parts)]:
                    for status in [0,1]:
                        for fragment in [7,65536]: cases.append((layout,stop,status,fragment))
        for index,(layout,stop,status,fragment) in enumerate(cases):
            data=payloads[layout];pair=[]
            for label,client in zip(['original','native'],clients):
                target=base/f'target-{index}-{label}'
                process=subprocess.Popen([str(client),str(target),str(stop),str(status),'32'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env={**env,'PIPE_FIXTURE_SIZE':str(len(data))},bufsize=0)
                channel=process.stdin;process.stdin=None
                def feed():
                    try:
                        for start in range(0,len(data),fragment):
                            remaining=memoryview(data)[start:start+fragment]
                            while remaining: remaining=remaining[channel.write(remaining):]
                    except BrokenPipeError: pass
                    finally: channel.close()
                writer=threading.Thread(target=feed,daemon=True);writer.start();stdout,stderr=process.communicate(timeout=30);writer.join(timeout=2)
                pair.append({'exit':process.returncode,'output':stdout.decode(),'tree':snapshot(target,aborted=stop!=0 or 'missing' in layout) if target.exists() else []})
            row={'layout':layout,'stop_part':stop,'invalid_status':status,'fragment':fragment,'original':pair[0],'native':pair[1]};results.append(row)
            if pair[0]!=pair[1]: mismatches.append(row)
        payload_hashes={name:hashlib.sha256(data).hexdigest() for name,data in payloads.items()}
    output=pathlib.Path('docs/wimlib/evidence/native-ffi-pipe-extract/part-differential.json')
    output.write_text(json.dumps({'cases':len(cases),'exact':len(cases)-len(mismatches),'library_sha256':dict(zip(['original','native'],libraries)),'parts':part_evidence,'payload_sha256':payload_hashes,'results':results},indent=2)+'\n')
    print(f'{len(cases)} cases; {len(mismatches)} mismatches; {output}')
    return bool(mismatches)
if __name__=='__main__': raise SystemExit(main())
