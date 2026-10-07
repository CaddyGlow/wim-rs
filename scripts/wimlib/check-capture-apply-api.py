#!/usr/bin/env python3
"""Compare real native capture/write and independently apply both WIM outputs."""
import argparse,hashlib,json,os,pathlib,runpy,subprocess,tempfile

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--original',default='/tmp/wimlib-native-oracle');parser.add_argument('--native',default='target/debug');parser.add_argument('--output',default='docs/wimlib/evidence/native-ffi-capture/apply-differential.json');args=parser.parse_args()
    env={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'};snapshot=runpy.run_path('scripts/wimlib/check-extract-api.py')['snapshot'];mismatches=[];observations=0
    with tempfile.TemporaryDirectory(prefix='capture-apply-api-') as temporary:
        base=pathlib.Path(temporary);probes=[]
        for label,library in [('original',pathlib.Path(args.original)/'.libs'),('native',pathlib.Path(args.native))]:
            probe=base/('probe-'+label);subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-api.c','-L',str(library),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(probe)],check=True);probes.append(probe)
        common=base/'common';common.mkdir()
        subprocess.run(['bash','-c','''
            fixture_index=0
            msg() { fixture_label="$*"; }
            do_test() {
                fixture="$fixture_root/case-$fixture_index"
                mkdir "$fixture"
                (cd "$fixture"; eval "$1") || exit 1
                printf '%s\\t%s\\n' "$fixture_index" "$fixture_label" >> "$fixture_root/cases.tsv"
                fixture_index=$((fixture_index + 1))
            }
            source "$srcdir/tests/common_tests.sh"
        '''],env={**env,'fixture_root':str(common),'srcdir':'/tmp/wimlib'},check=True)
        fixtures=[(common/('case-'+line.split('\t')[0]),line.split('\t')[1])for line in(common/'cases.tsv').read_text().splitlines()]
        cases=[(source,label,flags)for source,label in fixtures for flags in[0,4,0x10,0x14]]
        for index,(source,label,flags)in enumerate(cases):
            outputs=[];trees=[];apply_codes=[]
            before=snapshot(source)
            for library,probe in zip(['original','native'],probes):
                for path in[source,*source.rglob('*')]:os.utime(path,ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100),follow_symlinks=False)
                wim=base/f'{index}-{library}.wim';target=base/f'{index}-{library}-apply'
                run=subprocess.run([str(probe),str(source),str(flags),'NULL',str(wim),'0','0'],env=env,capture_output=True)
                outputs.append((run.returncode,run.stdout.decode()))
                if run.stdout.endswith(b'write 0\n'):
                    result=subprocess.run([str(pathlib.Path(args.original)/'wimlib-imagex'),'apply',str(wim),'1',str(target),*(['--unix-data']if flags&0x10 else[])],env=env,capture_output=True)
                    apply_codes.append(result.returncode);trees.append(snapshot(target))
                else:apply_codes.append(None);trees.append([])
            observations+=len(outputs[0][1].splitlines())
            if outputs[0]!=outputs[1]or apply_codes[0]!=apply_codes[1]or trees[0]!=trees[1]:mismatches.append({'case':index,'fixture':label,'flags':flags,'original_output':outputs[0],'native_output':outputs[1],'apply_codes':apply_codes,'original_tree':trees[0],'native_tree':trees[1]})
            # Payload bytes and link topology are preserved; reading capture streams changes access time.
            after=snapshot(source)
            for nodes in[before,after]:
                for node in nodes:node.pop('mtime',None);node.pop('atime',None)
            if before!=after:raise RuntimeError('capture changed input filesystem payloads')
    output=pathlib.Path(args.output);output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps({'cases':len(cases),'observations':observations,'source_payloads_preserved':True,'mismatches':mismatches},indent=2)+'\n');print(f'{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches; {output}');return bool(mismatches)
if __name__=='__main__':raise SystemExit(main())
