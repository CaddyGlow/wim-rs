#!/usr/bin/env python3
"""Compare fixed header error precedence to wimlib's real public open API.

Requires a prebuilt native header_status example and a built upstream library.
Only valid baseline/early header errors are compared: native probe is NOT open_wim.
"""
import argparse
import ctypes
from pathlib import Path
import struct
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--oracle', required=True, type=Path)
    parser.add_argument('--native', required=True, type=Path)
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--pipable-fixture', type=Path)
    args = parser.parse_args()
    lib = ctypes.CDLL(str(args.oracle.resolve()))
    lib.wimlib_open_wim.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_open_wim.restype = ctypes.c_int
    lib.wimlib_free.argtypes = [ctypes.c_void_p]
    original = args.fixture.read_bytes()
    # Expected codes independently enumerated in upstream public wimlib.h.
    cases = [('valid_baseline', original, 0)]
    for name, offset, fmt, value, expected in [
        ('bad_magic', 0, '<Q', 0, 43),
        ('bad_header_size', 8, '<I', 207, 17),
        ('unknown_version', 12, '<I', 1, 67),
        ('part_zero', 40, '<H', 0, 25),
        ('parts_zero', 42, '<H', 0, 25),
        ('part_exceeds_total', 40, '<H', 2, 25),
        ('image_count_exceeds_limit', 44, '<I', 65536, 10),
        ('oversized_blob_table', 64, '<Q', len(original) + 1, 17),
        ('oversized_xml', 88, '<Q', len(original) + 1, 17),
        ('oversized_integrity', 140, '<Q', len(original) + 1, 17),
        ('missing_compression_algorithm', 16, '<I', 2, 16),
        ('uncompressed_nonzero_chunk', 20, '<I', 32768, 15),
    ]:
        data = bytearray(original)
        struct.pack_into(fmt, data, offset, value)
        cases.append((name, data, expected))
    for name, flags, chunk in [
        ('xpress_small_chunk', 2 | 0x20000, 2048),
        ('lzx_nonpower_chunk', 2 | 0x40000, 32769),
        ('lzms_large_chunk', 2 | 0x80000, 2147483648),
    ]:
        data = bytearray(original)
        struct.pack_into('<II', data, 16, flags, chunk)
        cases.append((name, data, 15))
    # Both implementations must read the entire fixed header before checking magic.
    cases.extend((f'truncated_{n}', original[:n], 65) for n in (0, 7, 100, 207))
    if args.pipable_fixture:
        pipable = args.pipable_fixture.read_bytes()
        cases.append(('pipable_baseline', pipable, 0))
        data = bytearray(pipable)
        data[8:24] = bytes(16)
        cases.append(('pipable_placeholder_fields_ignored', data, 0))
        data = bytearray(pipable)
        data[-208:-200] = bytes([0xfa]) * 8
        cases.append(('pipable_final_magic_not_revalidated', data, 0))
        data = bytearray(pipable)
        struct.pack_into('<I', data, len(data) - 208 + 12, 1)
        cases.append(('pipable_final_version_validated', data, 67))
    with tempfile.TemporaryDirectory(prefix='wim-header-differential-') as directory:
        for name, data, expected in cases:
            path = Path(directory) / (name + '.wim')
            path.write_bytes(data)
            handle = ctypes.c_void_p()
            oracle = lib.wimlib_open_wim(bytes(path), 0, ctypes.byref(handle))
            if handle.value:
                lib.wimlib_free(handle)
            output = subprocess.check_output([str(args.native.resolve()), str(path)], text=True)
            native = int(output.split()[0])
            if oracle != expected or native != oracle:
                raise SystemExit(f'{name}: expected={expected} oracle={oracle} native={native}')
            print(f'PASS {name}: {native}')
    print(f'{len(cases)} differential header cases passed')


if __name__ == '__main__':
    main()
