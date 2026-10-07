#!/usr/bin/env python3
"""Differential unchanged-header clients, decoding original-compressor blocks."""
import argparse
import ctypes as C
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source',type=Path,default=Path('/tmp/wimlib'))
p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'))
p.add_argument('--native',type=Path,required=True)
a=p.parse_args()
lib=C.CDLL(str(a.original/'libwim.so'))
lib.wimlib_create_compressor.argtypes=[C.c_int,C.c_size_t,C.c_uint,C.POINTER(C.c_void_p)]
lib.wimlib_compress.argtypes=[C.c_void_p,C.c_size_t,C.c_void_p,C.c_size_t,C.c_void_p]
lib.wimlib_compress.restype=C.c_size_t
lib.wimlib_free_compressor.argtypes=[C.c_void_p]
records=[]
with tempfile.TemporaryDirectory(prefix='wim-decompress-abi-') as directory:
    directory=Path(directory)
    clients=[]
    for name,library in [('original',a.original),('native',a.native)]:
        binary=directory/name
        subprocess.run(['cc','-I'+str(a.source/'include'),'scripts/wimlib/probe-decompress-api.c','-L'+str(library.resolve()),'-Wl,-rpath,'+str(library.resolve()),'-lwim','-o',str(binary)],check=True)
        clients.append(binary)
    for codec in [1,2,3]:
        maximum=32768
        compressor=C.c_void_p()
        assert lib.wimlib_create_compressor(codec,maximum,50,C.byref(compressor))==0
        for size in [64,257,4096,32768]:
            for pattern in [b'A',b'0123456789abcdefghijklmnopqrstuvwxyz',bytes(range(256))]:
                plain=(pattern*((size+len(pattern)-1)//len(pattern)))[:size]
                source=C.create_string_buffer(plain)
                packed=C.create_string_buffer(size*2+2048)
                length=lib.wimlib_compress(source,size,packed,len(packed),compressor)
                assert length>0
                path=directory/'block.bin'; path.write_bytes(packed.raw[:length])
                outputs=[subprocess.check_output([str(client),str(codec),str(path),str(size),str(maximum)]) for client in clients]
                assert outputs[0]==outputs[1],(codec,size,outputs)
                # Verify the client hash against independently known input, not only equal libraries.
                checksum=14695981039346656037
                for value in plain: checksum=((checksum^value)*1099511628211)&((1<<64)-1)
                assert f'decode 0 0 {checksum} 204'.encode() in outputs[1]
                records.append({'codec':codec,'size':size,'compressed_size':length,'client_sha256':hashlib.sha256(outputs[1]).hexdigest()})
        lib.wimlib_free_compressor(compressor)
print(json.dumps({'exports':3,'factory_cases_per_client':66,'blocks':len(records),'equal':True,'cases':records},indent=2))
