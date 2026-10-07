#!/usr/bin/env python3
"""Compare native metadata parsing against original C tree iteration.
Requires built metadata-oracle.c, metadata-client.rs and a --compress=none WIM.
The .bin fixture came from alias/link/sub/file captured with --unix-data.
"""
import hashlib
import pathlib
import subprocess
import tempfile
import sys

wim, c_client, rust_client = map(pathlib.Path, sys.argv[1:4])
b = bytearray(wim.read_bytes())
table = int.from_bytes(b[56:64], "little")
table_size = int.from_bytes(b[48:55], "little")
for row in range(table, table + table_size, 50):
    if b[row + 7] & 2:
        start = int.from_bytes(b[row + 8:row + 16], "little")
        length = int.from_bytes(b[row:row + 7], "little")
        break
else:
    raise ValueError("missing metadata record")
raw = b[start:start + length]
root = (int.from_bytes(raw[:4], "little") + 7) & ~7 or 8
children = int.from_bytes(raw[root + 16:root + 24], "little")

cases = [("baseline", [])]
for field, offset, width, values in [
    ("security_length", 0, 4, [0, 1, 7, 8, 16, length + 1]),
    ("security_count", 4, 4, [1, 0x80000001]),
    ("root_attributes", root + 8, 4, [0, 16, 1024]),
    ("root_length", root, 8, [0, 1, 8, 16, 101, 104, length + 1]),
    ("root_name_len", root + 100, 2, [1, 2, 65534]),
    ("root_stream_count", root + 96, 2, [1, 65535]),
    ("root_children", root + 16, 8, [0, length, length + 8]),
    ("child_security", children + 12, 4, [0, 0x7fffffff, 0xffffffff]),
    ("child_attributes", children + 8, 4, [0, 128, 16, 0x4000]),
    ("child_name_len", children + 100, 2, [0, 1, 65534]),
    ("child_group_id", children + 88, 8, [0, 1, 0xffffffffffffffff]),
]:
    for value in values:
        cases.append((f"{field}={value}", [(offset, value.to_bytes(width, "little"))]))
with tempfile.TemporaryDirectory(prefix="metadata-oracle-") as tmp:
    tmp = pathlib.Path(tmp)
    for label, edits in cases:
        data = bytearray(raw)
        for offset, value in edits:
            data[offset:offset + len(value)] = value
        modified = bytearray(b)
        modified[start:start + length] = data
        modified[row + 30:row + 50] = hashlib.sha1(data).digest()
        wim_path = tmp / "input.wim"
        bin_path = tmp / "input.bin"
        wim_path.write_bytes(modified)
        bin_path.write_bytes(data)
        c = subprocess.run([str(c_client), str(wim_path)], capture_output=True, text=True, timeout=10)
        rust = subprocess.run([str(rust_client), str(bin_path)], capture_output=True, text=True, timeout=10)
        if c.stdout != rust.stdout:
            raise AssertionError(f"{label}: C={c.stdout!r}, Rust={rust.stdout!r}, stderr={c.stderr!r}")
        print("PASS", label)
    print(f"matched {len(cases)} original-C/native-Rust tree cases")
