#!/usr/bin/env python3
"""Decode native Rust LZMS output with preserved original wimlib (test only)."""
import argparse
import ctypes
import hashlib
import json
import pathlib
import random
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle', default='/tmp/wimlib-native-oracle/.libs/libwim.so')
    parser.add_argument('--encoder', default='target/debug/examples/encode_lzms')
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    lib = ctypes.CDLL(args.oracle)
    lib.wimlib_create_decompressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_decompress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    lib.wimlib_free_decompressor.argtypes = [ctypes.c_void_p]
    rng = random.Random(0x4c5a4d53)
    cases = []
    for size in [4,5,16,17,18,32,255,256,257,1023,1024,1025,4096,32768,65536,131072]:
        cases += [(f'constant-{size}', b'A'*size), (f'random-{size}', rng.randbytes(size))]
    for period in [2,3,7,31,128,257,1024,4096]:
        p = rng.randbytes(period)
        cases.append((f'period-{period}', (p* (32768//period+1))[:32768]))
    for power in range(8):
        span = 1 << power
        cases.append((f'delta-power-{power}', bytes(((i//span)*37 + (i%span)*13)&255 for i in range(32768))))
    # Repeated call targets activate preprocessing, including wrapping offsets.
    for opcode in [b'\xe8', b'\xff\x15', b'\xf0\x83\x05', b'\x48\x8d\x05', b'\x4c\x8d\x05', b'\x48\x8b\x05', b'\xe9']:
        payload = bytearray(b'Z')
        for i in range(2000):
            position = len(payload)
            payload += opcode + ((0x12345678-position)&0xffffffff).to_bytes(4,'little') + b'\x90'
        cases.append((f'x86-{opcode.hex()}', bytes(payload)))
    for index in range(64):
        alphabet = rng.randrange(1,256)
        cases.append((f'alphabet-{index}',bytes(rng.randrange(alphabet) for _ in range(rng.randrange(100,32769)))))
    records=[]
    with tempfile.TemporaryDirectory(prefix='native-lzms-') as directory:
        source=pathlib.Path(directory)/'input';target=pathlib.Path(directory)/'encoded'
        for name,payload in cases:
            source.write_bytes(payload)
            subprocess.run([args.encoder,str(source),str(target),str(len(payload)*3+256)],check=True)
            encoded=target.read_bytes()
            assert encoded, name
            decoder=ctypes.c_void_p()
            assert lib.wimlib_create_decompressor(3,len(payload),ctypes.byref(decoder))==0,name
            try:
                output=ctypes.create_string_buffer(len(payload));# Upstream SIMD/word decoder reads padded guard memory; logical size
                # remains the exact compressed size (padding is not stream data).
                compressed=ctypes.create_string_buffer(encoded + bytes(64))
                result=lib.wimlib_decompress(compressed,len(encoded),output,len(payload),decoder)
                assert result==0,(name,result)
                assert output.raw==payload,name
            finally:
                lib.wimlib_free_decompressor(decoder)
            records.append({'case':name,'input_size':len(payload),'compressed_size':len(encoded),'input_sha256':hashlib.sha256(payload).hexdigest(),'compressed_sha256':hashlib.sha256(encoded).hexdigest(),'oracle_roundtrip':True})
    report={'oracle_revision':'cd5e231c348c255ae5088873b5a66ee0eb96fa07','cases':len(records),'records':records}
    pathlib.Path(args.output).write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'cases':len(records),'oracle_roundtrips':len(records)}))


if __name__ == '__main__':
    main()
