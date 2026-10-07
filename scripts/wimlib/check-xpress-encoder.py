#!/usr/bin/env python3
"""Verify native XPRESS output with the original wimlib decompressor.

The C library is a test-only oracle; the Rust production crate never links it.
Run after cargo build -p ms-compress --example xpress_encode in the native workspace.
"""
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
    parser.add_argument("--oracle", default="/tmp/wimlib-native-oracle/.libs/libwim.so")
    parser.add_argument("--encoder", default="target/debug/examples/xpress_encode")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    lib = ctypes.CDLL(args.oracle)
    lib.wimlib_create_decompressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_decompress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    lib.wimlib_free_decompressor.argtypes = [ctypes.c_void_p]
    lib.wimlib_create_compressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p)]
    lib.wimlib_compress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    lib.wimlib_compress.restype = ctypes.c_size_t
    lib.wimlib_free_compressor.argtypes = [ctypes.c_void_p]
    compressor = ctypes.c_void_p()
    assert lib.wimlib_create_compressor(1, 65536, 50, ctypes.byref(compressor)) == 0
    decoder = ctypes.c_void_p()
    assert lib.wimlib_create_decompressor(1, 65536, ctypes.byref(decoder)) == 0
    rng = random.Random(0x585052455353)
    cases = []
    for size in [0, 1, 24, 25, 260, 261, 273, 274, 1024, 32768, 65536]:
        cases.append((f"constant-{size}", bytes([0x93]) * size))
    for period in [1, 2, 3, 7, 16, 255, 256, 257, 1023, 4096, 16384, 32768]:
        pattern = rng.randbytes(period)
        cases.append((f"period-{period}", (pattern * (65536 // period + 1))[:65536]))
    for match_length in [3, 4, 17, 18, 19, 271, 272, 273, 274, 65535]:
        # A compressible suffix pays for the 256-byte Huffman table.
        pattern = rng.randbytes(min(match_length, 16384))
        cases.append((f"length-{match_length}", (pattern + pattern + b"Z" * 2000)[:65536]))
    for seed in range(256):
        size = rng.randrange(300, 65537)
        alphabet = 1 + seed % 64
        payload = bytes(rng.randrange(alphabet) for _ in range(size))
        cases.append((f"alphabet-{seed}", payload))
    for seed in range(64):
        size = rng.randrange(300, 65537)
        cases.append((f"random-{seed}", rng.randbytes(size)))
    cases = [(f"{name}-{policy}", payload, capacity)
             for name, payload in cases
             for policy, capacity in [("tight", len(payload)), ("generous", len(payload) * 8 + 1024)]]
    records = []
    capacity_observations = []
    capacity_input = ctypes.create_string_buffer(bytes([9]) * 4000)
    capacity_output = ctypes.create_string_buffer(10000)
    original_size = lib.wimlib_compress(capacity_input, 4000, capacity_output, 10000, compressor)
    for capacity in range(original_size - 1, original_size + 4):
        actual = lib.wimlib_compress(capacity_input, 4000, capacity_output, capacity, compressor)
        assert actual == (original_size if capacity >= original_size + 2 else 0)
        capacity_observations.append({"capacity": capacity, "original_returned_size": actual})
    try:
        with tempfile.TemporaryDirectory(prefix="wimlib-xpress-encoder-") as directory:
            source = pathlib.Path(directory) / "input"
            target = pathlib.Path(directory) / "output"
            for name, payload, capacity in cases:
                source.write_bytes(payload)
                subprocess.run([args.encoder, str(source), str(target), str(capacity)], check=True)
                compressed = target.read_bytes()
                record = {"case": name, "input_size": len(payload), "capacity": capacity, "input_sha256": hashlib.sha256(payload).hexdigest(), "compressed_size": len(compressed)}
                original_input = ctypes.create_string_buffer(payload)
                original_output = ctypes.create_string_buffer(capacity)
                original_size = lib.wimlib_compress(original_input, len(payload), original_output, capacity, compressor)
                record["original_compressed_size"] = original_size
                if compressed:
                    output = ctypes.create_string_buffer(len(payload))
                    encoded = ctypes.create_string_buffer(compressed)
                    assert lib.wimlib_decompress(encoded, len(compressed), output, len(payload), decoder) == 0, name
                    assert output.raw == payload, name
                    record["compressed_sha256"] = hashlib.sha256(compressed).hexdigest()
                    record["oracle_roundtrip"] = True
                else:
                    record["capacity_or_short_input"] = True
                records.append(record)
    finally:
        lib.wimlib_free_decompressor(decoder)
        lib.wimlib_free_compressor(compressor)
    report = {"oracle_revision": "cd5e231c348c255ae5088873b5a66ee0eb96fa07", "cases": len(records), "compressed_roundtrips": sum(bool(r.get("oracle_roundtrip")) for r in records), "expanded_roundtrips": sum(r["compressed_size"] > r["input_size"] for r in records), "capacity_observations": capacity_observations, "records": records}
    pathlib.Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "records"}))


if __name__ == "__main__":
    main()
