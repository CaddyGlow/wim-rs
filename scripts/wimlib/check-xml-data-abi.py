#!/usr/bin/env python3
"""Compare raw original XML access and C stdio extraction on Linux."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=Path('/tmp/wimlib'))
p.add_argument('--oracle', type=Path, default=Path('/tmp/wimlib-native-oracle'))
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
results = []
with tempfile.TemporaryDirectory(prefix='wim-xml-data-') as directory:
    root = Path(directory)
    clients = []
    for name, library in [('original', a.oracle / '.libs'), ('native', a.native)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-xml-data-api.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    tree = root / 'tree'
    tree.mkdir()
    (tree / 'payload').write_bytes(bytes(range(256)) * 512)
    for layout in ['ordinary', 'solid', 'pipable']:
        for codec in ['none', 'xpress', 'lzx', 'lzms']:
            if layout == 'solid' and codec == 'none': continue
            path = root / f'{layout}-{codec}.wim'
            options = ['--compress=' + codec] if layout != 'solid' else ['--solid', '--solid-compress=' + codec]
            if layout == 'pipable': options += ['--pipable']
            subprocess.run([str(a.oracle / 'wimlib-imagex'), 'capture', str(tree), str(path), 'Original 😀', *options, '--threads=1'], stdout=subprocess.DEVNULL, check=True)
            outputs = [subprocess.check_output([str(client), str(path)]) for client in clients]
            if outputs[0] != outputs[1]:
                differences = [(i, left[:120], right[:120], len(left), len(right)) for i, (left, right) in enumerate(zip(outputs[0].splitlines(), outputs[1].splitlines())) if left != right]
                raise AssertionError((layout, codec, differences))
            lines = outputs[0].decode().splitlines()
            original_xml = lines[1].split(':', 3)[3]
            edited_xml = next(line for line in lines if line.startswith('after-edit:')).split(':', 3)[3]
            assert original_xml == edited_xml
            stdio = next(line for line in lines if line.startswith('stdio:')).split(':', 1)[1]
            assert stdio == b'pre'.hex() + original_xml
            results.append({'layout': layout, 'codec': codec, 'lines': len(lines), 'exact_output_sha256': hashlib.sha256(outputs[0]).hexdigest()})
print(json.dumps({'passed': True, 'cases': len(results), 'results': results}, indent=2))
