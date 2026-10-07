#!/usr/bin/env python3
"""Compare native lookup-table resolution with the original public C API."""
import argparse
import hashlib
import json
import struct
import subprocess
import tempfile
from pathlib import Path


def entry(digest, flags, size, offset, uncompressed, references=1, part=1):
    return (size.to_bytes(7, "little") + bytes([flags]) +
            struct.pack("<QQHI", offset, uncompressed, part, references) + bytes([digest]) * 20)


def install_table(original, records, version=None, extra=b""):
    data = bytearray(original)
    if version is not None:
        struct.pack_into("<I", data, 12, version)
    data += extra
    table = b"".join(records)
    descriptor = len(table).to_bytes(7, "little") + b"\x02" + struct.pack("<QQ", len(data), len(table))
    data[48:72] = descriptor
    return data + table


def normalize(text):
    lines = text.strip().splitlines()
    status = lines[0]
    metadata = [line for line in lines[1:] if int(line.split()[4]) & 2]
    content = sorted(line for line in lines[1:] if not int(line.split()[4]) & 2)
    return status, metadata, content


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--source", type=Path, default=Path("/tmp/wimlib"))
    args = parser.parse_args()
    original = (args.source / "tests/wims/empty_dacl.wim").read_bytes()
    metadata, content = original[4666:4716], original[4716:4766]
    duplicate = bytearray(content)
    duplicate[:7] = (7).to_bytes(7, "little")
    struct.pack_into("<Q", duplicate, 16, 7)
    cases = [("original", original)]
    for name, records in [
        ("duplicate_first_wins", [metadata, content, duplicate]),
        ("zero_hash_ignored", [metadata, content, entry(0, 0, 7, 208, 7)]),
        ("zero_size_ignored", [metadata, content, entry(9, 0, 0, 208, 0)]),
        ("wrong_part_ignored", [metadata, content, entry(9, 0, 7, 208, 7, part=2)]),
        ("zero_metadata_refs", [metadata[:26] + b"\0" * 4 + metadata[30:], content]),
        ("extra_metadata_ignored", [metadata, metadata, content]),
        ("shared_metadata_rejected", [metadata[:26] + struct.pack("<I", 2) + metadata[30:], content]),
        ("uncompressed_size_mismatch", [metadata, entry(9, 0, 7, 208, 8)]),
        ("zero_hash_still_size_validated", [metadata, entry(0, 0, 7, 208, 8)]),
        ("solid_flag_ignored_in_old_version", [metadata, entry(9, 0x10, 7, 208, 7)]),
        ("partial_record_ignored", [metadata, content, b"\xfa" * 49]),
        ("no_metadata_reconciles_images", [content]),
    ]:
        cases.append((name, install_table(original, records)))
    offset = len(original)
    alternate = struct.pack("<QII", 20, 32768, 3)
    marker = entry(0, 0x10, 50, offset, 0x100000000)
    for name, blobs in [
        ("solid_valid", [entry(9, 0x10, 5, 2, 0)]),
        ("solid_disjoint_out_of_order", [entry(9, 0x10, 5, 10, 0), entry(8, 0x10, 5, 0, 0)]),
        ("solid_overlap", [entry(9, 0x10, 5, 0, 0), entry(8, 0x10, 5, 4, 0)]),
        ("solid_range_exceeds_resource", [entry(9, 0x10, 21, 0, 0)]),
        ("solid_metadata_rejected", [entry(9, 0x12, 5, 0, 0)]),
        ("solid_missing_resource", [entry(9, 0x10, 5, 0, 0)]),
    ]:
        records = [metadata] + ([] if name == "solid_missing_resource" else [marker]) + blobs
        cases.append((name, install_table(original, records, 0xe00, alternate)))
    fixtures = Path("crates/wim-format/tests/fixtures")
    for name in ("xpress-resource", "pipable-resource", "solid-resource"):
        cases.append((name, (fixtures / (name + ".wim")).read_bytes()))
    # Matching public projections, not internal HashMap iteration order.
    results = []
    with tempfile.TemporaryDirectory(prefix="wim-lookup-differential-") as directory:
        for name, data in cases:
            path = Path(directory) / (name + ".wim")
            path.write_bytes(data)
            oracle = subprocess.check_output([str(args.oracle.resolve()), str(path)], text=True)
            native = subprocess.check_output([str(args.native.resolve()), str(path)], text=True)
            if normalize(oracle) != normalize(native):
                raise SystemExit(f"mismatch {name}:\nC:\n{oracle}\nRust:\n{native}")
            results.append({"case": name, "input_sha256": hashlib.sha256(data).hexdigest(), "status": oracle.splitlines()[0]})
    print(json.dumps({"cases": len(results), "results": results}, indent=2))


if __name__ == "__main__":
    main()
