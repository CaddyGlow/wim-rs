#!/usr/bin/env python3
"""Prove shared pending metadata materialization through chained C exports."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser()
parser.add_argument('--oracle',type=pathlib.Path,default=pathlib.Path('/tmp/wimlib-native-oracle'))
args=parser.parse_args()
def run(command):
    return subprocess.run([str(value) for value in command],check=True,capture_output=True,text=True).stdout

def metadata(path):
    data=path.read_bytes()
    offset=int.from_bytes(data[56:64],'little')
    size=int.from_bytes(data[48:55],'little')
    entries=[]
    for cursor in range(offset,offset+size,50):
        record=data[cursor:cursor+50]
        if record[7]&2:
            resource_size=int.from_bytes(record[:7],'little')
            resource_offset=int.from_bytes(record[8:16],'little')
            body=data[resource_offset:resource_offset+resource_size]
            assert hashlib.sha1(body).digest()==record[30:50]
            entries.append(body)
    assert len(entries)==1
    return entries[0]

with tempfile.TemporaryDirectory(prefix='wim-shared-export-') as temporary:
    temp=pathlib.Path(temporary)
    cases=[]
    for label,library in [('original',args.oracle/'.libs'),('native',ROOT/'target/debug')]:
        binary=temp/label
        run(['cc','-I/tmp/wimlib/include',ROOT/'scripts/wimlib/probe-shared-export-metadata.c','-L'+str(library),'-Wl,-rpath,'+str(library),'-lwim','-o',binary])
        outputs=[temp/f'{label}-{owner}.wim' for owner in ['a','b','c']]
        observations=run([binary,*outputs]).splitlines()
        raw=[metadata(path) for path in outputs]
        for path in outputs:run([args.oracle/'wimlib-imagex','verify',path])
        cases.append({'library':label,'observations':observations,'written_metadata_sizes':[len(value) for value in raw],'three_written_metadata_resources_equal':raw[0]==raw[1]==raw[2],'written_metadata_hashes':[hashlib.sha1(value).hexdigest() for value in raw],'original_verify_outputs':len(outputs)})
    matches=cases[0]['observations']==cases[1]['observations']
    def normalize_filetimes(raw):
        # SecurityTable::parse rounds the security length to eight bytes and
        # treats lengths below eight as an empty eight-byte header.
        root_offset=max(8,(int.from_bytes(raw[:4],'little')+7)&~7)
        normalized=bytearray(raw)
        normalized[root_offset+40:root_offset+64]=bytes(24)
        return bytes(normalized)
    original_metadata=metadata(temp/'original-a.wim')
    native_metadata=metadata(temp/'native-a.wim')
    layout_equal=normalize_filetimes(original_metadata)==normalize_filetimes(native_metadata)
    canonical_sizes=all(size==128 for case in cases for size in case['written_metadata_sizes'])
    result={'observations_equal':matches,'observation_count':len(cases[0]['observations']),'written_metadata_equal_after_filetime_normalization':layout_equal,'filetime_normalization':'root_offset+40..root_offset+64, three FILETIME fields only','canonical_pending_metadata_size_128':canonical_sizes,'cases':cases}
    print(json.dumps(result,indent=2))
    if not matches or not layout_equal or not canonical_sizes or not all(case['three_written_metadata_resources_equal'] for case in cases):raise SystemExit(1)
