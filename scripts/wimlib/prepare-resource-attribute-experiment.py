#!/usr/bin/env python3
"""Copy bounded ACE18 buffers into a scratch-only experiment input; no source mutation."""
import argparse
import hashlib
import json
import struct
from pathlib import Path


def validate_ace18(raw):
    if len(raw) < 28 or raw[0] != 18 or struct.unpack_from('<H', raw, 2)[0] != len(raw):
        raise ValueError('invalid resource attribute ACE envelope')
    if raw[1] & ~31 or struct.unpack_from('<I', raw, 4)[0] != 0:
        raise ValueError('unsupported ACE flags or access mask')
    if raw[8] != 1 or raw[9] > 15:
        raise ValueError('invalid SID')
    claim = 8 + 8 + 4 * raw[9]
    if claim + 16 > len(raw):
        raise ValueError('truncated claim')
    name, kind, reserved, flags, count = struct.unpack_from('<IHHII', raw, claim)
    if reserved or kind != 2 or count != 1 or flags != 0:
        raise ValueError('experiment accepts only source UINT64 single-value claims')
    value = struct.unpack_from('<I', raw, claim + 16)[0] if claim + 20 <= len(raw) else 0
    if name % 2 or value % 8:
        raise ValueError('misaligned claim-relative name or UINT64 value')
    for offset, size in ((name, 2), (value, 8)):
        if offset < 20 or claim + offset + size > len(raw):
            raise ValueError('claim-relative offset outside ACE')
    end = claim + name
    while end + 2 <= len(raw) and raw[end:end + 2] != b'\0\0':
        end += 2
    if end + 2 > len(raw):
        raise ValueError('unterminated claim name')
    raw[claim + name:end].decode('utf-16-le', errors='strict')
    return raw


def prepare(source):
    data = source.read_bytes()
    rows = json.loads(data)['rows']
    aces = []
    for row in rows:
        for encoded in row['reserved_aces']:
            raw = bytes.fromhex(encoded)
            if raw and raw[0] == 18:
                validate_ace18(raw)
                aces.append({'ace_hex': raw.hex(), 'source_path_utf16_le': row['path_utf16_le']})
    if len(aces) != 2:
        raise ValueError('expected exactly two frozen source ACE18 challenges')
    return {'schema': 1, 'source_inventory_sha256': hashlib.sha256(data).hexdigest(),
            'executed': False, 'aces': aces}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inventory', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = prepare(args.inventory)
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
