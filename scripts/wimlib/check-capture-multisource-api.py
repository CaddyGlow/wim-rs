#!/usr/bin/env python3
"""Original/native multisource, add-tree and actual ADD update comparison."""
import argparse,json,os,pathlib,runpy,subprocess,tempfile

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--original',default='/tmp/wimlib-native-oracle');parser.add_argument('--native',default='target/debug');parser.add_argument('--output',default='docs/wimlib/evidence/native-ffi-capture/multisource-differential.json');args=parser.parse_args()
    env={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'};snapshot=runpy.run_path('scripts/wimlib/check-extract-api.py')['snapshot'];mismatches=[];observations=0
    with tempfile.TemporaryDirectory(prefix='capture-multisource-')as temporary:
        base=pathlib.Path(temporary);probes=[]
        for label,library in [('original',pathlib.Path(args.original)/'.libs'),('native',pathlib.Path(args.native))]:
            probe=base/('probe-'+label);subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-multisource-api.c','-L',str(library),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(probe)],check=True);probes.append(probe)
        a=base/'a';b=base/'b';a.mkdir();b.mkdir();(a/'data').write_bytes(b'alpha');(a/'empty').touch();(b/'data').write_bytes(b'beta');os.link(a/'empty',b/'empty');os.link(a/'data',b/'alias');(a/'nested').mkdir();(a/'nested/deep').write_bytes(b'deep');os.symlink('data',a/'link')
        pairs=[[],[(a,'/')],[(a,'/'),(b,'/')],[(a,'/A'),(b,'/B')],[(a/'data','/filler/deep/file')],[(a/'data','/same'),(b/'data','/same')],[(a/'data','/same'),(b,'/same')],[(a,'/same'),(b/'data','/same')],[(a/'empty','/one'),(b/'empty','/two')],[(a/'data','/one'),(b/'alias','/two')],[(a,'NULL')],[(a,'\\a\\b')],[(base/'missing','/')],[(a,'/'),(base/'missing','/missing')]]
        cases=[(mode,flags,status,pair)for mode in[0,1,2,3,4]for flags in[0,4,0x10,0x14,0x2000,0x2004,0x100,8]for status in[0]for pair in pairs if mode!=1 or len(pair)==1]
        cases.extend((2,4|0x4000,status,pair)for status in[109,110,111,121,122,123,130,131,209,210,211,221,222,223,230,231,300,400]for pair in[pairs[2],pairs[5],pairs[12]])
        for index,(mode,flags,status,pair)in enumerate(cases):
            outputs=[];trees=[];codes=[]
            for label,probe in zip(['original','native'],probes):
                for source in[a,b]:
                    for path in[source,*source.rglob('*')]:os.utime(path,ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100),follow_symlinks=False)
                wim=base/f'{index}-{label}.wim';target=base/f'{index}-{label}-apply';arguments=[str(probe),str(mode),str(flags),str(status),'NULL',str(wim)]
                for source,destination in pair:arguments.extend([str(source),destination])
                run=subprocess.run(arguments,env=env,capture_output=True);outputs.append((run.returncode,run.stdout.decode()))
                if run.stdout.endswith(b'write 0\n'):
                    result=subprocess.run([str(pathlib.Path(args.original)/'wimlib-imagex'),'apply',str(wim),'1',str(target),*(['--unix-data']if flags&0x10 else[])],env=env,capture_output=True);codes.append(result.returncode);trees.append(snapshot(target,aborted=True))
                else:codes.append(None);trees.append([])
            observations+=len(outputs[0][1].splitlines())
            if outputs[0]!=outputs[1]or codes[0]!=codes[1]or trees[0]!=trees[1]:mismatches.append({'case':index,'mode':mode,'flags':flags,'status':status,'sources':[(str(p.relative_to(base)),t)for p,t in pair],'original_output':outputs[0],'native_output':outputs[1],'apply_codes':codes,'original_tree':trees[0],'native_tree':trees[1]})
    output=pathlib.Path(args.output);output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps({'cases':len(cases),'observations':observations,'normalization':'current wall-clock timestamps on synthetic filler directories only','mismatches':mismatches},indent=2)+'\n');print(f'{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches; {output}');return bool(mismatches)
if __name__=='__main__':raise SystemExit(main())
