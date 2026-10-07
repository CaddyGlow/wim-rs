#!/usr/bin/env python3
"""Unchanged-header scan/state comparison; writer payload gate is separate."""
import argparse,json,os,pathlib,subprocess,tempfile

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--original',default='/tmp/wimlib-native-oracle');parser.add_argument('--native',default='target/debug');parser.add_argument('--output',default='docs/wimlib/evidence/native-ffi-capture/scan-differential.json');args=parser.parse_args()
    env=os.environ.copy();env['WIMLIB_DISABLE_CPU_FEATURES']='sse4.2'
    cases=[];mismatches=[];observations=0
    with tempfile.TemporaryDirectory(prefix='capture-scan-api-') as temporary:
        base=pathlib.Path(temporary);probes=[]
        for label,library in [('original',pathlib.Path(args.original)/'.libs'),('native',pathlib.Path(args.native))]:
            probe=base/label;subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-api.c','-L',str(library),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(probe)],check=True);probes.append(probe)
        source=base/'source';source.mkdir();(source/'data').write_bytes(b'initial data');os.link(source/'data',source/'alias');(source/'dir').mkdir();os.symlink('data',source/'link');os.symlink(str(source/'dir'),source/'absolute');os.mkfifo(source/'fifo');os.setxattr(source/'data',b'user.example',b'value')
        raw=base/'raw';raw.mkdir();open(os.fsencode(raw)+b'/\xed\xa0\x80','wb').write(b'raw')
        bad=base/'bad';bad.mkdir();open(os.fsencode(bad)+b'/\xff','wb').write(b'bad')
        configs={'none':'NULL'}
        for label,data in {'exclude':b'[ExclusionList]\n/data\n/dir\n','exception':b'[ExclusionList]\n*\n[ExclusionException]\n/data\n','invalid':b'[ExclusionList]\nrelative/path\n','quote':b'[ExclusionList]\n"/data"\n','unknown':b'[Ignored]\n/data\n','utf16':b'\xff\xfe'+('[ExclusionList]\n/data\n').encode('utf-16le')}.items():
            path=base/(label+'.ini');path.write_bytes(data);configs[label]=str(path)
        for src in [source,source/'data',source/'link',source/'missing',raw,bad]:
            for flags in [0,4,0x10,0x14,2,0x12,0x104,0x204,0x300,0x400,0x404,0x800,0x804,0x1000,0x1014,0x2000,0x4000,0x8000,0x10000,0x20000,8,1]:
                cases.append((src,flags,'none',0))
        for config in configs:
            for flags in [0,4,0x14,0x800,0x1000,0x1800]:cases.append((source,flags,config,0))
        for status in [109,110,111,209,210,211,130,230]:cases.append((source,4|0x4000,'none',status))
        for index,(src,flags,config,status) in enumerate(cases):
            outputs=[]
            for probe in probes:
                run=subprocess.run([str(probe),str(src),str(flags),configs[config],str(base/'output.wim'),str(status),'0'],env=env,capture_output=True)
                lines=run.stdout.splitlines();prefix=[]
                for line in lines:
                    if line.startswith((b'write',b'progress 13',b'progress 14')):break
                    prefix.append(line)
                outputs.append((run.returncode,prefix))
            observations+=len(outputs[0][1])
            if outputs[0]!=outputs[1]:mismatches.append({'case':index,'source':str(src.relative_to(base)),'flags':flags,'config':config,'status':status,'original':str(outputs[0]),'native':str(outputs[1])})
    output=pathlib.Path(args.output);output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps({'scope':'scan/state only; writer payload compatibility separately required','cases':len(cases),'observations':observations,'mismatches':mismatches},indent=2)+'\n');print(f'{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches; {output}');return bool(mismatches)
if __name__=='__main__':raise SystemExit(main())
