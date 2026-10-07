#!/usr/bin/env python3
"""Compare native add/delete image state with original unchanged-header clients."""
import argparse
import json
import pathlib
import subprocess
import tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser()
parser.add_argument('--oracle',type=pathlib.Path,default=pathlib.Path('/tmp/wimlib-native-oracle'))
args=parser.parse_args()
def run(command):
    return subprocess.run([str(v) for v in command],check=True,capture_output=True,text=True).stdout

def normalize(text):
    # Blob hash-table enumeration order is unspecified; compare complete records
    # as a sorted multiset within each snapshot. All state/action lines stay ordered.
    result=[]
    resources=[]
    for line in text.splitlines():
        if line.startswith('resource '): resources.append(line)
        else:
            if resources:
                result.extend(sorted(resources))
                resources=[]
            result.append(line)
    result.extend(sorted(resources))
    return result

with tempfile.TemporaryDirectory(prefix='wim-image-mutation-') as temporary:
    temp=pathlib.Path(temporary)
    binaries=[]
    for name,lib in [('original',args.oracle/'.libs'),('native',ROOT/'target/debug')]:
        binary=temp/name
        run(['cc','-I/tmp/wimlib/include',ROOT/'scripts/wimlib/probe-image-mutation.c','-L'+str(lib),'-Wl,-rpath,'+str(lib),'-lwim','-o',binary])
        binaries.append(binary)
    tool=args.oracle/'wimlib-imagex'
    source=temp/'source'
    source.mkdir()
    (source/'shared').write_bytes(b'owned image mutations\n'*800)
    fixtures=[('new',[])]
    for codec in ('none','XPRESS','LZX','LZMS'):
        path=temp/(codec+'.wim')
        run([tool,'capture',source,path,'First','--compress='+codec])
        run([tool,'append',source,path,'Second'])
        run([tool,'append',source,path,'Third','--boot'])
        fixtures.append((codec,[path]))
    later_part=temp/'later-part.wim'
    data=bytearray((temp/'none.wim').read_bytes())
    data[40:44]=b'\x02\x00\x02\x00'
    later_part.write_bytes(data)
    fixtures.append(('later-part',[later_part]))
    # Existing malformed metadata is deliberately checked lazily during deletion.
    for name in ['cyclic.wim','corrupted_file_1.wim','corrupted_file_2.wim','dotdot.wim','duplicate_names.wim','linux_xattrs_old.wim','empty_dacl.wim']:
        fixtures.append((name,[pathlib.Path('/tmp/wimlib/tests/wims')/name]))
    output={'cases':[],'failures':[]}
    for label,paths in fixtures:
        original=normalize(run([binaries[0],*paths]))
        native=normalize(run([binaries[1],*paths]))
        differences=[{'line':i,'original':a,'native':b} for i,(a,b) in enumerate(zip(original,native),1) if a!=b]
        if len(original)!=len(native): differences.append({'original_lines':len(original),'native_lines':len(native)})
        output['cases'].append({'fixture':label,'observations':len(original),'matches':not differences})
        if differences:output['failures'].append({'fixture':label,'differences':differences[:30]})
    print(json.dumps(output,indent=2))
    if output['failures']:raise SystemExit(1)
