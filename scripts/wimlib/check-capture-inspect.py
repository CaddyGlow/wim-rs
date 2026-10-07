#!/usr/bin/env python3
"""Compare unhashed graph lookup and traversal using the original public header."""
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
results = []
with tempfile.TemporaryDirectory(prefix='wim-capture-inspect-') as directory:
    root = Path(directory)
    frozen = root / 'library'
    frozen.mkdir()
    shutil.copy2(a.native / 'libwim.so', frozen / 'libwim.so')
    digest = hashlib.sha256((frozen / 'libwim.so').read_bytes()).hexdigest()
    clients = []
    for name, library in [('original', Path('/tmp/wimlib-native-oracle/.libs')), ('native', frozen)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                        'scripts/wimlib/probe-capture-inspect.c', '-L' + str(library),
                        '-Wl,-rpath,' + str(library), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    for hardlinks, flags, stop in itertools.product([False, True], [0, 16], [0, 1, 2, 3, 4, 5]):
        tree = root / f'tree-{hardlinks}-{flags}-{stop}'
        tree.mkdir()
        (tree / 'dir').mkdir()
        (tree / 'dir' / 'child').write_bytes(b'child')
        (tree / 'a').write_bytes(b'shared stream')
        if hardlinks:
            os.link(tree / 'a', tree / 'z')
        else:
            (tree / 'z').write_bytes(b'shared stream')
        (tree / 'empty').touch()
        before = {str(path.relative_to(tree)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in tree.rglob('*') if path.is_file()}
        outputs = [subprocess.check_output([str(client), str(tree), str(flags), str(stop)])
                   for client in clients]
        assert outputs[0] == outputs[1], (hardlinks, flags, stop, outputs)
        after = {str(path.relative_to(tree)): hashlib.sha256(path.read_bytes()).hexdigest()
                 for path in tree.rglob('*') if path.is_file()}
        assert before == after
        results.append({'hardlinks': hardlinks, 'flags': flags, 'stop': stop,
                        'stdout': outputs[0].decode()})
print(json.dumps({'cases': len(results), 'native_sha256': digest, 'results': results}, indent=2))
