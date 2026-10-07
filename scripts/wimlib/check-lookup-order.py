#!/usr/bin/env python3
"""Literal public C collision/growth/reference/early-stop comparison; no sorting."""
import argparse
import hashlib
import json
import os
import struct
import subprocess
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--original', type=Path, required=True)
p.add_argument('--native', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
base = Path('/tmp/wimlib/tests/wims/empty_dacl.wim').read_bytes()
metadata, existing = base[4666:4716], base[4716:4766]
# Real payload digests all collide under masks 63 and 127, while bit 8 varies.
payloads = []
i = 0
while len(payloads) < 130:
    payload = ('real lookup collision payload %d' % i).encode()
    digest = hashlib.sha1(payload).digest()
    if digest[0] & 127 == 0:
        payloads.append((payload, digest))
    i += 1
results = []
for count in (1, 62, 63, 64, 65, 66, 126, 127, 128, 129, 130):
    for extra_metadata in (0, 3):
        data = bytearray(base)
        records = [metadata, existing] + [metadata] * extra_metadata
        for payload, digest in payloads[:count]:
            offset = len(data)
            data += payload
            records.append(len(payload).to_bytes(7, 'little') + b'\0' +
                           struct.pack('<QQHI', offset, len(payload), 1, 1) + digest)
        table = b''.join(records)
        data[48:72] = len(table).to_bytes(7, 'little') + b'\x02' + struct.pack('<QQ', len(data), len(table))
        data += table
        fixture = a.output / ('collision-%d-extra-%d.wim' % (count, extra_metadata))
        fixture.write_bytes(data)
        for reference in (0, 1):
            outputs = []
            for library in (a.original, a.native):
                env = dict(os.environ, LD_LIBRARY_PATH=str(library.parent))
                r = subprocess.run([str(library), str(fixture), str(reference)], env=env,
                                   capture_output=True, text=True, check=True)
                outputs.append(r.stdout)
            name = '%d-%d-%d' % (count, extra_metadata, reference)
            for tag, text in zip(('original', 'native'), outputs):
                (a.output / (name + '-' + tag + '.txt')).write_text(text)
            results.append(dict(case=name, raw_entries=len(records), reference=bool(reference),
                                fixture_sha256=hashlib.sha256(data).hexdigest(), equal=outputs[0] == outputs[1]))
(a.output / 'results.json').write_text(json.dumps(dict(
    caller_sha256=hashlib.sha256(Path('scripts/wimlib/probe-lookup-order.c').read_bytes()).hexdigest(),
    fixture_scope='original metadata and existing payload retained plus real collision payloads; lookup traversal only',
    results=results), indent=2) + '\n')
print('%d/%d literal outcomes equal' % (sum(r['equal'] for r in results), len(results)))
raise SystemExit(not all(r['equal'] for r in results))
