#!/usr/bin/env python3
"""Check real source checksums and error precedence before capture export commit."""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
rows = []
with tempfile.TemporaryDirectory(prefix='wim-export-errors-') as directory:
    root = Path(directory)
    frozen = root / 'library'
    frozen.mkdir()
    shutil.copy2(a.native / 'libwim.so', frozen / 'libwim.so')
    digest = hashlib.sha256((frozen / 'libwim.so').read_bytes()).hexdigest()
    clients = []
    for name, library in [('original', Path('/tmp/wimlib-native-oracle/.libs')), ('native', frozen)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                        'scripts/wimlib/probe-capture-export.c', '-L' + str(library),
                        '-Wl,-rpath,' + str(library), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    for mutation, flags in itertools.product([1, 2, 3], [0, 2, 8]):
        outputs = []
        for index, client in enumerate(clients):
            tree = root / f'tree-{mutation}-{flags}-{index}'
            tree.mkdir()
            (tree / 'file').write_bytes(b'export source' * 3000)
            outputs.append(subprocess.check_output([str(client), str(tree), str(tree / 'output.wim'), '0', str(flags), str(mutation)]).decode())
        rows.append({'mutation': mutation, 'flags': flags, 'original': outputs[0], 'native': outputs[1], 'equal': outputs[0] == outputs[1]})
print(json.dumps({'cases': len(rows), 'equal': sum(row['equal'] for row in rows), 'native_sha256': digest, 'results': rows}, indent=2))
raise SystemExit(any(not row['equal'] for row in rows))
