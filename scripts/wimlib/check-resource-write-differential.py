#!/usr/bin/env python3
"""Frame chunks with Rust, relocate into C-captured WIMs, extract with original C.

C compression is only a test input generator. Native serializer production code
has no C dependency. Both raw and genuinely compressed chunks are exercised.
"""
import argparse
import ctypes
import hashlib
import json
import random
import struct
import subprocess
import tempfile
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--oracle', type=Path, required=True)
p.add_argument('--native', type=Path, required=True)
p.add_argument('--library', type=Path, required=True)
a = p.parse_args()
lib = ctypes.CDLL(str(a.library.resolve()))
lib.wimlib_create_compressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)]
lib.wimlib_compress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
lib.wimlib_compress.restype = ctypes.c_size_t
lib.wimlib_free_compressor.argtypes = [ctypes.c_void_p]

def descriptor(size, flags, offset, usize):
    return size.to_bytes(7,'little') + bytes([flags]) + struct.pack('<QQ',offset,usize)

def command(args):
    subprocess.run([str(x) for x in args], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

results = []
with tempfile.TemporaryDirectory(prefix='wim-resource-write-') as temp:
    root = Path(temp)
    source = root/'source'; source.mkdir()
    chunk_size = 32768
    payload = b'A'*chunk_size + random.Random(0x57494D).randbytes(chunk_size) + b'partial last chunk'
    (source/'payload.bin').write_bytes(payload)
    digest = hashlib.sha1(payload).digest()
    base = root/'base.wim'
    command([a.oracle.resolve(),'capture',source,base,'--compress=None','--no-acls','--nocheck'])
    original = base.read_bytes()
    table_size = int.from_bytes(original[48:55],'little')
    table_offset = struct.unpack_from('<Q',original,56)[0]
    table = original[table_offset:table_offset+table_size]
    records = [table[i:i+50] for i in range(0,len(table),50)]
    target = next(i for i,r in enumerate(records) if r[30:50] == digest)
    for codec, name in [(1,'XPRESS'),(2,'LZX'),(3,'LZMS')]:
        for raw in [False,True]:
            chunks = root/'chunks'; chunks.mkdir(exist_ok=True)
            compressor = ctypes.c_void_p()
            assert lib.wimlib_create_compressor(codec,chunk_size,0,ctypes.byref(compressor)) == 0
            try:
                for i,start in enumerate(range(0,len(payload),chunk_size)):
                    chunk = payload[start:start+chunk_size]
                    out = ctypes.create_string_buffer(len(chunk))
                    size = 0 if raw else lib.wimlib_compress(chunk,len(chunk),out,len(chunk)-1,compressor)
                    (chunks/str(i)).write_bytes(out.raw[:size])
            finally:
                lib.wimlib_free_compressor(compressor)
            for layout in ['ordinary','pipable','solid']:
                resource = root/'resource'
                command([a.native.resolve(),source/'payload.bin',codec,chunk_size,layout,chunks,resource])
                serialized = resource.read_bytes()
                archive = bytearray(original)
                offset = len(archive)
                archive += serialized
                updated = list(records)
                if layout == 'solid':
                    marker = descriptor(len(serialized),16,offset,1<<32) + struct.pack('<HI',1,1) + bytes(20)
                    blob = descriptor(len(payload),16,0,0) + records[target][24:]
                    updated[target:target+1] = [marker,blob]
                    struct.pack_into('<I',archive,12,0xe00)
                else:
                    updated[target] = descriptor(len(serialized),4,offset,len(payload)) + records[target][24:]
                new_table = b''.join(updated)
                new_offset = len(archive); archive += new_table
                archive[48:72] = descriptor(len(new_table),0,new_offset,len(new_table))
                # Header compression flags from upstream header.c.
                flags = struct.unpack_from('<I',archive,16)[0]
                flags &= ~(0x20000|0x40000|0x80000)
                flags |= 2 | {1:0x20000,2:0x40000,3:0x80000}[codec]
                struct.pack_into('<II',archive,16,flags,chunk_size)
                if layout == 'pipable':
                    archive[:8] = b'WLPWM\0\0\0'
                    archive += archive[:208]
                wim = root/'framed.wim'; wim.write_bytes(archive)
                extracted = root/'output'; extracted.mkdir(exist_ok=True)
                command([a.oracle.resolve(),'extract',wim,'1','payload.bin',f'--dest-dir={extracted}','--no-acls'])
                assert (extracted/'payload.bin').read_bytes() == payload, (name,layout,raw)
                results.append({'codec':name,'layout':layout,'raw_chunks':raw,'payload_bytes':len(payload),'resource_bytes':len(serialized),'resource_sha256':hashlib.sha256(serialized).hexdigest()})
print(json.dumps({'cases':len(results),'results':results},indent=2))
