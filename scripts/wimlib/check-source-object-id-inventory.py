#!/usr/bin/env python3
"""Cross-check complete native ObjectIDs against independent raw $ObjId:$O bytes."""
import argparse
import json
import struct
from pathlib import Path


def u16(data, offset):
    return struct.unpack_from('<H', data, offset)[0]


def restore_usa(record, sector_size=512):
    if len(record) < 48 or record[:4] != b'INDX' or len(record) % sector_size:
        raise ValueError('invalid INDX record geometry/signature')
    offset, count = struct.unpack_from('<HH', record, 4)
    if count != len(record)//sector_size+1 or offset < 40 or offset+count*2 > len(record):
        raise ValueError('invalid index update-sequence bounds')
    output = bytearray(record)
    signature = record[offset:offset+2]
    for index in range(1, count):
        tail = index*sector_size-2
        if record[tail:tail+2] != signature:
            raise ValueError('index update-sequence mismatch')
        output[tail:tail+2] = record[offset+index*2:offset+index*2+2]
    return bytes(output)


def entries(data, header):
    if header+16 > len(data):
        raise ValueError('short index header')
    start, used, allocated = struct.unpack_from('<III', data, header)
    if start < 16 or used < start or used > allocated or header+allocated > len(data):
        raise ValueError('index header bounds')
    position, end = header+start, header+used
    output = []
    terminal = False
    while position < end:
        if position+16 > end:
            raise ValueError('short index entry')
        data_offset, data_length = struct.unpack_from('<HH', data, position)
        size, key_length, flags = struct.unpack_from('<HHH', data, position+8)
        if size < 16 or size % 8 or position+size > end or flags & ~3:
            raise ValueError('index entry bounds or flags')
        trailer = 8 if flags & 1 else 0
        if flags & 2:
            if key_length or data_length or size != 16+trailer:
                raise ValueError('invalid terminal index entry')
            terminal = True
            position += size
            break
        if key_length != 16 or data_length != 56 or data_offset < 32 or data_offset+data_length > size-trailer:
            raise ValueError('invalid ObjectID key/value bounds')
        key = data[position+16:position+32]
        value = data[position+data_offset:position+data_offset+56]
        output.append({'object_id': key.hex(), 'file_reference': int.from_bytes(value[:8], 'little'), 'full64': (key+value[8:]).hex()})
        position += size
    if not terminal or position != end:
        raise ValueError('index terminal entry missing or trailing bytes')
    return output


def read_index(audit):
    record = next(r for r in audit['records'] if r['path'] == '$Extend/$ObjId')
    attrs = {a['type']: bytes.fromhex(a['raw_hex']) for a in record['attributes'] if a.get('name') == '$O' and 'raw_hex' in a}
    root = attrs['IndexRoot']
    if len(root) < 32:
        raise ValueError('short ObjectID root')
    block = int.from_bytes(root[8:12], 'little')
    if block < 512 or block > 65536 or block % 512:
        raise ValueError('index block size outside bounds')
    rows = entries(root, 16)
    allocation = attrs.get('IndexAllocation', b'')
    bitmap = attrs.get('Bitmap', b'')
    if len(allocation) % block or len(allocation) > 64*1024*1024:
        raise ValueError('index allocation size outside bounds')
    count = len(allocation)//block
    if len(bitmap)*8 < count or any(bitmap[bit//8] & (1 << (bit%8)) for bit in range(count, len(bitmap)*8)):
        raise ValueError('index bitmap outside allocation')
    for index in range(count):
        if bitmap[index//8] & (1 << (index%8)):
            rows.extend(entries(restore_usa(allocation[index*block:(index+1)*block]), 24))
    if len({r['object_id'] for r in rows}) != len(rows):
        raise ValueError('duplicate ObjectID index key')
    return rows


def compare(inventory, audit):
    rows = read_index(audit)
    actual = {r['file_reference']: r for r in rows}
    if len(actual) != len(rows):
        raise ValueError('multiple ObjectIDs for one file reference')
    observed = set()
    for group in inventory['groups']:
        reference = group['file_reference']
        if reference in observed or len(bytes.fromhex(group['object_id_full64'])) != 64:
            raise ValueError('duplicate group or incomplete ObjectID')
        observed.add(reference)
        if actual.get(reference, {}).get('full64') != group['object_id_full64']:
            raise ValueError('native ObjectID differs independent raw index')
    internal = [r for r in rows if r['file_reference'] not in observed]
    if any((r['file_reference'] & ((1<<48)-1)) >= 16 for r in internal):
        raise ValueError('ordinary indexed ObjectID omitted from inventory')
    return {'raw_index_entries': len(rows), 'ordinary_file_groups': len(observed), 'ordinary_paths': inventory['object_id_paths'], 'internal_index_entries_excluded': internal, 'all_ordinary_full64_exact_independent_raw_index': True}


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inventory', type=Path);parser.add_argument('audit', type=Path);parser.add_argument('new_output', type=Path)
    args=parser.parse_args()
    report=compare(json.loads(args.inventory.read_bytes()),json.loads(args.audit.read_bytes()))
    with args.new_output.open('x') as output:json.dump(report,output,indent=2)
    print(json.dumps(report))
