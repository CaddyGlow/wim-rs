#!/usr/bin/env python3
"""Compare native handle API with unchanged original header and original library."""
import argparse
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--oracle', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-native-oracle'))
args = parser.parse_args()

def run(command):
    return subprocess.run([str(x) for x in command], check=True, capture_output=True, text=True).stdout

with tempfile.TemporaryDirectory(prefix='wim-handle-api-') as temporary:
    temp = pathlib.Path(temporary)
    source = temp / 'source'
    source.mkdir()
    (source / 'payload').write_bytes(b'native handles contract\n' * 3000)
    paths = []
    tool = args.oracle / 'wimlib-imagex'
    for codec in ('none', 'XPRESS', 'LZX', 'LZMS'):
        path = temp / (codec + '.wim')
        run([tool, 'capture', source, path, 'Handle fixture', '--compress=' + codec, '--check'])
        paths.append(path)
    baseline = paths[0].read_bytes()
    mutations = {'bad-magic': (0,b'INVALID!'), 'bad-version': (12,(0x99).to_bytes(4,'little')), 'bad-parts':(40,b'\0\0'), 'readonly':(16,(4).to_bytes(4,'little')), 'boot-invalid':(120,(9).to_bytes(4,'little')), 'bad-count':(44,(2).to_bytes(4,'little')), 'incomplete':(48,bytes(48)), 'bad-xml':(72,bytes(24)), 'bad-lookup':(48,bytes(24)), 'invalid-chunk':(20,(1).to_bytes(4,'little')), 'split':(42,(2).to_bytes(2,'little'))}
    for name,(offset,data) in mutations.items():
        modified=bytearray(baseline)
        modified[offset:offset+len(data)]=data
        path=temp/(name+'.wim')
        path.write_bytes(modified)
        paths.append(path)
    truncated=temp/'truncated.wim'
    truncated.write_bytes(baseline[:200])
    paths.extend([truncated,temp/'missing.wim',source])
    # Supplied original regression fixtures exercise malformed lookup/XML files too.
    paths.extend(sorted(pathlib.Path('/tmp/wimlib/tests/wims').glob('*.wim')))
    original=temp/'original'
    native=temp/'native'
    probe=ROOT/'scripts/wimlib/probe-handles-api.c'
    original_lib=args.oracle/'.libs'
    native_lib=ROOT/'target/debug'
    for output,library in ((original,original_lib),(native,native_lib)):
        run(['cc','-I/tmp/wimlib/include',probe,'-L'+str(library),'-Wl,-rpath,'+str(library),'-lwim','-o',output])
    expected=run([original,*paths]).splitlines()
    actual=run([native,*paths]).splitlines()
    failures=[{'original':a,'native':b} for a,b in zip(expected,actual) if a!=b]
    result={'observations':len(expected),'fixture_files':len(paths),'fixtures':[p.name for p in paths],'line_counts_equal':len(expected)==len(actual),'failures':failures}
    print(json.dumps(result,indent=2))
    if failures or len(expected)!=len(actual):
        raise SystemExit(1)
