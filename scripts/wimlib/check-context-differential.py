#!/usr/bin/env python3
"""Compare native codec configuration with the preserved wimlib factory.

Successful compressor allocations use tiny blocks to avoid a gigabyte-scale
memory gate. Full-sized decoder factories allocate codec tables, not output.
The Rust compressor projection validates configuration only: this does not
establish encoder level tuning, allocation hooks, or memory-query parity.
"""
import argparse
import ctypes
import json
import pathlib
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", default="/tmp/wimlib-native-oracle/.libs/libwim.so")
    parser.add_argument("--probe", default="target/debug/examples/context_probe")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    lib = ctypes.CDLL(args.library)
    lib.wimlib_create_compressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_create_decompressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_free_compressor.argtypes = [ctypes.c_void_p]
    lib.wimlib_free_decompressor.argtypes = [ctypes.c_void_p]
    lib.wimlib_get_compressor_needed_memory.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.c_uint]
    lib.wimlib_get_compressor_needed_memory.restype = ctypes.c_uint64
    cases = []
    for codec in [-2, -1, 0, 4, 2147483647]:
        cases.extend((codec, maximum, level) for maximum in [0, 1] for level in [0, 0x01000000])
    for codec, maximum in [(1, 65536), (2, 1 << 21), (3, 1 << 30)]:
        cases.extend((codec, size, level) for size in [0, 1, maximum + 1] for level in [0, 1, 50, 0xFFFFFF, 0x1000000, 0x80000000, 0x80FFFFFF, 0xFFFFFFFF])
    results = []
    for codec, maximum, level in cases:
        handle = ctypes.c_void_p()
        dec = lib.wimlib_create_decompressor(codec, maximum, ctypes.byref(handle))
        lib.wimlib_free_decompressor(handle)
        handle = ctypes.c_void_p()
        comp = lib.wimlib_create_compressor(codec, maximum, level, ctypes.byref(handle))
        lib.wimlib_free_compressor(handle)
        native = list(map(int, subprocess.check_output([args.probe, str(codec), str(maximum), str(level)], text=True).split()))
        assert native == [dec, comp], (codec, maximum, level, native, dec, comp)
        results.append(dict(codec=codec, maximum=maximum, level=level, decoder=dec, compressor_config=comp))
    boundaries = []
    for codec, maximum in [(1, 65536), (2, 1 << 21), (3, 1 << 30)]:
        handle = ctypes.c_void_p()
        dec = lib.wimlib_create_decompressor(codec, maximum, ctypes.byref(handle))
        lib.wimlib_free_decompressor(handle)
        assert dec == 0
        for level in [0, 1, 50, 0xFFFFFF, 0x80000000, 0x80FFFFFF]:
            memory = lib.wimlib_get_compressor_needed_memory(codec, maximum, level)
            assert memory != 0
            native = list(map(int, subprocess.check_output([args.probe, str(codec), str(maximum), str(level)], text=True).split()))
            assert native == [0, 0]
            boundaries.append(dict(codec=codec, maximum=maximum, level=level, decoder=dec, original_compressor_memory=memory, native_configuration_valid=True))
    document = dict(source_commit="cd5e231c348c255ae5088873b5a66ee0eb96fa07", scope="factory configuration validation; compressor creation allocations not reproduced; boundary memory values recorded, not matched", cases=results, maximum_boundary_cases=boundaries)
    destination = pathlib.Path(args.output)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(document, indent=2) + "\n")
    print(f"{len(results)} original/native factory comparisons and {len(boundaries)} maximum-boundary validations passed")


if __name__ == "__main__":
    main()
