#!/usr/bin/env python3
"""Compare deferred captured extraction, callbacks, and subsequent write."""
import json, os, pathlib, runpy, subprocess, tempfile

def main():
    snapshot = runpy.run_path('scripts/wimlib/check-extract-api.py')['snapshot']
    env = {**os.environ, 'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
    mismatches = []; observations = 0
    with tempfile.TemporaryDirectory(prefix='capture-direct-extract-') as temporary:
        base = pathlib.Path(temporary); source = base / 'source'; source.mkdir()
        (source/'a').write_bytes(b'alpha'*12345); (source/'b').write_bytes(b'beta'*23456)
        os.link(source/'a', source/'alias'); (source/'duplicate').write_bytes((source/'a').read_bytes())
        (source/'empty').touch(); (source/'link').symlink_to('a'); (source/'absolute').symlink_to(source/'b')
        probes = []
        for label, library in [('original',pathlib.Path('/tmp/wimlib-native-oracle/.libs')),('native',pathlib.Path('target/debug'))]:
            probe = base / label
            subprocess.run(['cc','-I','/tmp/wimlib/include','scripts/wimlib/probe-capture-api.c','-L',str(library),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(probe)],check=True)
            probes.append(probe)
        cases = [(flags,status) for flags in [0,32,256,512,1024] for status in [0,100,103,104,106,107,200,204]]
        for index,(flags,status) in enumerate(cases):
            results = []
            for label,probe in zip(['original','native'],probes):
                for path in [source,*source.iterdir()]: os.utime(path,ns=(1600000000123456700,1600000001765432100),follow_symlinks=False)
                target = base/f'{index}-{label}-target'
                run = subprocess.run([str(probe),str(source),'20','NULL',str(base/f'{index}-{label}.wim'),str(status),'0'],env={**env,'CAPTURE_EXTRACT_TARGET':str(target),'CAPTURE_EXTRACT_FLAGS':str(flags)},capture_output=True)
                results.append({'exit':run.returncode,'output':run.stdout.decode(),'tree':snapshot(target, aborted=status != 0) if target.exists() else []})
            observations += len(results[0]['output'].splitlines())
            if results[0] != results[1]: mismatches.append({'flags':flags,'status':status,'original':results[0],'native':results[1]})
    output = pathlib.Path('docs/wimlib/evidence/native-ffi-capture/direct-extract-differential.json')
    output.write_text(json.dumps({'cases':len(cases),'observations':observations,'mismatches':mismatches},indent=2)+'\n')
    print(f'{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches')
    return bool(mismatches)
if __name__ == '__main__': raise SystemExit(main())
