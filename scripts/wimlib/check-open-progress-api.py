#!/usr/bin/env python3
"""Compare original open integrity events, cancellation and output ownership."""
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
with tempfile.TemporaryDirectory(prefix='wim-open-progress-') as directory:
    root = Path(directory)
    clients = []
    for name, library in [('original', a.oracle / '.libs'), ('native', a.native)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-open-progress-api.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    fixture = Path('crates/wim-format/tests/fixtures/integrity-small-chunks.wim').read_bytes()
    integrity_offset = struct.unpack_from('<Q', fixture, 132)[0]
    assert integrity_offset > 208
    variants = {'valid': fixture}
    for name, offset in [('first-mismatch', 208), ('last-mismatch', integrity_offset - 1), ('bad-table', integrity_offset), ('bad-xml', struct.unpack_from('<Q', fixture, 80)[0])]:
        changed = bytearray(fixture)
        changed[offset] ^= 1
        variants[name] = bytes(changed)
    absent = bytearray(fixture)
    absent[124:148] = bytes(24)
    variants['absent-table'] = bytes(absent)
    variants['truncated-table'] = fixture[:-1]
    variants['short-header'] = fixture[:207]
    for name, data in variants.items():
        path = root / (name + '.wim')
        path.write_bytes(data)
        before = hashlib.sha256(data).hexdigest()
        for flags in [0, 1, 3, 5, 7, 8, -1]:
            for stop, status in [(0, 0), (1, 1), (2, 1), (3, -1), (5, 2), (6, 1)]:
                outputs = [subprocess.check_output([str(client), str(path), str(flags), str(stop), str(status)]) for client in clients]
                assert outputs[0] == outputs[1], (name, flags, stop, status, outputs)
                results.append({'fixture': name, 'flags': flags, 'stop': stop, 'status': status, 'observations': len(outputs[0].splitlines()), 'output_sha256': hashlib.sha256(outputs[0]).hexdigest()})
        assert hashlib.sha256(path.read_bytes()).hexdigest() == before
print(json.dumps({'passed': True, 'cases': len(results), 'input_files_preserved': True, 'results': results}, indent=2))
