#!/usr/bin/env python3
"""Compare native verification on valid archives and independent corruptions."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=Path('/tmp/wimlib'))
p.add_argument('--oracle', type=Path, default=Path('/tmp/wimlib-native-oracle'))
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
results = []
with tempfile.TemporaryDirectory(prefix='wim-verify-abi-') as directory:
    root = Path(directory)
    clients = []
    for name, library in [('original', a.oracle / '.libs'), ('native', a.native)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-verify-api.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    tree = root / 'tree'
    tree.mkdir()
    (tree / 'data').write_bytes(bytes(range(256)) * 300)
    (tree / 'other').write_bytes(b'blob integrity verification\n' * 2000)
    imagex = a.oracle / 'wimlib-imagex'
    cases = []
    for layout in ['ordinary', 'solid', 'pipable']:
        for codec in ['none', 'xpress', 'lzx', 'lzms']:
            if layout == 'solid' and codec == 'none': continue
            path = root / f'{layout}-{codec}.wim'
            options = ['--compress=' + codec] if layout != 'solid' else ['--solid', '--solid-compress=' + codec]
            if layout == 'pipable': options += ['--pipable']
            if layout == 'ordinary' and codec == 'none': options += ['--check']
            subprocess.run([str(imagex), 'capture', str(tree), str(path), 'Original', *options, '--threads=1'], stdout=subprocess.DEVNULL, check=True)
            cases.append((path.stem, path))
    base = (root / 'ordinary-none.wim').read_bytes()
    table_size = int.from_bytes(base[48:55], 'little')
    table_offset = struct.unpack_from('<Q', base, 56)[0]
    records = list(range(table_offset, table_offset + table_size, 50))
    metadata_record = next(offset for offset in records if base[offset + 7] & 2)
    data_record = next(offset for offset in records if not base[offset + 7] & 2)
    metadata_offset = struct.unpack_from('<Q', base, metadata_record + 8)[0]
    metadata_size = struct.unpack_from('<Q', base, metadata_record + 16)[0]
    for kind in ['data-hash', 'metadata-hash', 'security-structure', 'missing-stream', 'truncated-payload', 'integrity-table-not-verified']:
        data = bytearray(base)
        if kind == 'data-hash':
            data[struct.unpack_from('<Q', data, data_record + 8)[0]] ^= 1
        elif kind == 'metadata-hash':
            data[metadata_offset + metadata_size - 1] ^= 1
        elif kind == 'security-structure':
            struct.pack_into('<I', data, metadata_offset + 4, 0x80000001)
        elif kind == 'missing-stream':
            security_length = max(8, (struct.unpack_from('<I', data, metadata_offset)[0] + 7) & ~7)
            first_child = struct.unpack_from('<Q', data, metadata_offset + security_length + 16)[0]
            data[metadata_offset + first_child + 64: metadata_offset + first_child + 84] = b'\xa5' * 20
        elif kind == 'truncated-payload':
            struct.pack_into('<Q', data, data_record + 8, len(data) + 1)
        else:
            integrity_offset = struct.unpack_from('<Q', data, 132)[0]
            assert integrity_offset != 0
            data[integrity_offset + 12] ^= 1
        if kind in ['security-structure', 'missing-stream']:
            data[metadata_record + 30:metadata_record + 50] = hashlib.sha1(data[metadata_offset:metadata_offset + metadata_size]).digest()
        path = root / (kind + '.wim')
        path.write_bytes(data)
        cases.append((kind, path))
    for name, path in cases:
        outputs = [subprocess.check_output([str(client), str(path)], text=True) for client in clients]
        assert outputs[0] == outputs[1], (name, outputs)
        results.append({'case': name, 'result': outputs[0].splitlines()})
print(json.dumps({'passed': True, 'cases': len(results), 'results': results}, indent=2))
