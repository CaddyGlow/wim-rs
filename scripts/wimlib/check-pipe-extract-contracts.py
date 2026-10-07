#!/usr/bin/env python3
"""Preserve original pipe API reference contracts independently of native results."""
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import struct
import tempfile

fixture = Path('crates/wim-format/tests/fixtures/pipable-resource.wim')
ordinary = Path('crates/wim-format/tests/fixtures/xpress-resource.wim')
rows = []
with tempfile.TemporaryDirectory(prefix='wim-pipe-contracts-') as directory:
    root = Path(directory)
    client = root / 'original'
    subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                    'scripts/wimlib/probe-pipe-extract.c', '-L/tmp/wimlib-native-oracle/.libs',
                    '-Wl,-rpath,/tmp/wimlib-native-oracle/.libs', '-lwim', '-o', str(client)], check=True)
    env = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
    payloads = {'pipable': fixture.read_bytes(), 'ordinary': ordinary.read_bytes(),
                'empty': b'', 'header-short': fixture.read_bytes()[:100],
                'xml-short': fixture.read_bytes()[:250]}
    def changed(name, offset, replacement):
        value = bytearray(payloads['pipable'])
        value[offset:offset + len(replacement)] = replacement
        payloads[name] = bytes(value)
    changed('part-two-first', 40, struct.pack('<HH', 2, 2))
    changed('image-count-two', 44, struct.pack('<I', 2))
    changed('invalid-xml-frame-magic', 208, b'badframe')
    changed('xml-not-metadata', 244, struct.pack('<I', 0))
    changed('xml-empty', 216, struct.pack('<Q', 0))
    changed('xml-hash-zero', 224, bytes(20))
    changed('xml-invalid-text', 248, b'\xff\xff\xff\xff')
    for layout, image, flags, stop in itertools.product(payloads, ['NULL', '1', '0', 'all', 'missing'], [0, 4, -1], [-1, 0, 103, 105, 107, 203]):
        target = root / f'target-{len(rows)}'
        result = subprocess.run([str(client), image, str(target), str(flags), str(stop)], input=payloads[layout], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, check=True)
        rows.append({'layout': layout, 'image': image, 'flags': flags, 'stop': stop,
                     'stdout': result.stdout.decode(), 'target_created': target.exists(),
                     'files': sorted(str(p.relative_to(target)) for p in target.rglob('*')) if target.exists() else []})
print(json.dumps({'source_commit': 'cd5e231c348c255ae5088873b5a66ee0eb96fa07',
                  'scope': 'original reference contracts only; no native comparison in this record',
                  'cases': len(rows), 'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(),
                  'results': rows}, indent=2))
