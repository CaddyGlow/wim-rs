"""Prepare bounded source-ACE challenges; no source disk writes or guest execution."""
import argparse
import hashlib
import json
from pathlib import Path


def prepare(inventory_bytes, expected_inventory_sha256, helper_bytes):
    if len(inventory_bytes) > 256 * 1024 * 1024 or hashlib.sha256(inventory_bytes).hexdigest() != expected_inventory_sha256:
        raise ValueError('Source inventory bounds/hash mismatch')
    source = json.loads(inventory_bytes)
    chosen = {}
    for row in source['rows']:
        sd = bytes.fromhex(row['security'])
        if len(sd) < 20 or sd[0] != 1 or not int.from_bytes(sd[2:4], 'little') & 0x8000:
            raise ValueError('Source SD header')
        sacl = int.from_bytes(sd[12:16], 'little')
        if sacl < 20 or sacl % 4 or sacl + 8 > len(sd):
            raise ValueError('Source SACL bounds')
        size = int.from_bytes(sd[sacl+2:sacl+4], 'little')
        if size < 8 or sacl + size > len(sd):
            raise ValueError('Source SACL length')
        cursor = sacl + 8
        actual = []
        for _ in range(int.from_bytes(sd[sacl+4:sacl+6], 'little')):
            if cursor + 4 > sacl + size:
                raise ValueError('Source ACE header')
            length = int.from_bytes(sd[cursor+2:cursor+4], 'little')
            if length < 4 or length % 4 or cursor + length > sacl + size:
                raise ValueError('Source ACE bounds')
            if sd[cursor] in (18, 20):
                actual.append(sd[cursor:cursor+length].hex())
            cursor += length
        if actual != row['reserved_aces']:
            raise ValueError('Raw source SACL/reserved inventory mismatch')
        for text in actual:
            chosen.setdefault(text, dict(name='source-ace'+str(bytes.fromhex(text)[0])+'-'+str(len(chosen)),
                ace_hex=text, acl_revision=sd[sacl], source_path_utf16_le=row['path_utf16_le'],
                source_sd_sha256=hashlib.sha256(sd).hexdigest(), source_file_reference=row['file_id']))
    if not 1 <= len(chosen) <= 16:
        raise ValueError('Distinct reserved challenge count outside bound')
    return dict(schema=1, mode='scratch-backup-security',
                source_inventory_sha256=expected_inventory_sha256,
                helper_sha256=hashlib.sha256(helper_bytes).hexdigest(),
                scope='Copied distinct opaque reserved ACE variants only; guest retains its own scratch SID/ACL context. No reserved behavior proven.',
                challenges=list(chosen.values()))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('inventory', type=Path); parser.add_argument('expected_sha256')
    parser.add_argument('helper', type=Path); parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = prepare(args.inventory.read_bytes(), args.expected_sha256, args.helper.read_bytes())
    with args.output.open('x') as stream:
        json.dump(result, stream, indent=2); stream.write('\n')
    print(hashlib.sha256(args.output.read_bytes()).hexdigest())


if __name__ == '__main__':
    main()
