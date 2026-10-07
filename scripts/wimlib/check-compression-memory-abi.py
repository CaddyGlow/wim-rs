#!/usr/bin/env python3
"""Compare original and native allocation-query ABI without allocating huge buffers."""
import argparse
import ctypes
import json
import random

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("original")
parser.add_argument("native")
args = parser.parse_args()

def load(path):
    lib = ctypes.CDLL(path)
    query = lib.wimlib_get_compressor_needed_memory
    query.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.c_uint]
    query.restype = ctypes.c_uint64
    setter = lib.wimlib_set_default_compression_level
    setter.argtypes = [ctypes.c_int, ctypes.c_uint]
    setter.restype = ctypes.c_int
    return query, setter

original, original_set = load(args.original)
native, native_set = load(args.native)
sizes = [0, 1, 2, 15, 32767, 32768, 32769, 65535, 65536, 65537,
         65786, 65787, 65788, 2097151, 2097152, 2097153,
         (1 << 26) - 1, 1 << 26, (1 << 26) + 1,
         (1 << 30) - 1, 1 << 30, (1 << 30) + 1, ctypes.c_size_t(-1).value]
levels = [0, 1, 34, 35, 50, 59, 60, 100, 0xffffff, 0x1000000,
          0x7fffffff, 0x80000000, 0x80000001, 0x80000022,
          0x80000023, 0x8000003b, 0x8000003c, 0x80ffffff, 0xffffffff]
queries = 0
setters = 0
samples = []

def compare(codec, size, level):
    global queries
    expected = original(codec, size, level)
    actual = native(codec, size, level)
    assert actual == expected, (codec, size, level, expected, actual)
    queries += 1
    if len(samples) < 12 and expected:
        samples.append([codec, size, level, expected])

for codec in [-2147483648, -1, 0, 1, 2, 3, 4, 2147483647]:
    for size in sizes:
        for level in levels:
            compare(codec, size, level)
randomizer = random.Random(0x57494d)
for _ in range(2000):
    compare(randomizer.randrange(-1, 5), randomizer.randrange(1 << 31),
            randomizer.choice(levels + [randomizer.randrange(1 << 32)]))
for codec in [-1, 1, 2, 3, 0, 4, -2]:
    for default in [0, 1, 34, 35, 59, 60, 0xffffff, 0x1000000, 0x80000000, 0xffffffff]:
        expected = original_set(codec, default)
        actual = native_set(codec, default)
        assert actual == expected, ("setter", codec, default, expected, actual)
        setters += 1
        for queried in [1, 2, 3]:
            for size in sizes:
                for level in [0, 1, 35, 60, 0x80000000]:
                    compare(queried, size, level)
original_set(-1, 0)
native_set(-1, 0)
print(json.dumps({"queries_matched": queries, "setters_matched": setters,
                  "samples": samples, "pointer_bits": ctypes.sizeof(ctypes.c_void_p) * 8,
                  "native_memory_budget_verified": False}, indent=2))
