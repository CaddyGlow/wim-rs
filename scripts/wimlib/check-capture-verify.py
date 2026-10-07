#!/usr/bin/env python3
"""Compare verification of original-backed capture streams before and after write."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
rows = []
with tempfile.TemporaryDirectory(prefix='wim-capture-verify-') as directory:
    root = Path(directory)
    frozen = root / 'library'
    frozen.mkdir()
    shutil.copy2(a.native / 'libwim.so', frozen / 'libwim.so')
    digest = hashlib.sha256((frozen / 'libwim.so').read_bytes()).hexdigest()
    clients = []
    for name, library in [('original', Path('/tmp/wimlib-native-oracle/.libs')), ('native', frozen)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                        'scripts/wimlib/probe-capture-verify.c', '-L' + str(library),
                        '-Wl,-rpath,' + str(library), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    for size, mode, alias in itertools.product([19, 32768, 65537], range(4), ['none', 'hardlink', 'duplicate']):
        outputs = []
        for index, client in enumerate(clients):
            tree = root / f'tree-{size}-{mode}-{alias}-{index}'
            tree.mkdir()
            (tree / 'file').write_bytes(b'x' * size)
            if alias == 'hardlink':
                os.link(tree / 'file', tree / 'alias')
            elif alias == 'duplicate':
                (tree / 'alias').write_bytes(b'x' * size)
            outputs.append(subprocess.check_output([str(client), str(tree), str(tree / 'out.wim'), str(mode)]).decode())
        rows.append({'size': size, 'mode': mode, 'alias': alias, 'original': outputs[0], 'native': outputs[1], 'equal': outputs[0] == outputs[1]})
print(json.dumps({'cases': len(rows), 'equal': sum(row['equal'] for row in rows), 'native_sha256': digest, 'results': rows}, indent=2))
raise SystemExit(any(not row['equal'] for row in rows))
