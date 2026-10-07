#!/usr/bin/env python3
"""Compare resource callback fields and cancellation with original wimlib."""
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
with tempfile.TemporaryDirectory(prefix='wim-lookup-abi-') as directory:
    root = Path(directory)
    clients = []
    for name, library in [('original', a.oracle / '.libs'), ('native', a.native)]:
        binary = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-lookup-api.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(binary)], check=True)
        clients.append(binary)
    tree = root / 'tree'
    tree.mkdir()
    (tree / 'first').write_bytes(bytes(range(256)) * 257)
    (tree / 'second').write_bytes(b'another compressed content block\n' * 3000)
    (tree / 'duplicate').write_bytes((tree / 'first').read_bytes())
    imagex = a.oracle / 'wimlib-imagex'
    for layout in ['ordinary', 'solid', 'pipable']:
        for codec in ['none', 'xpress', 'lzx', 'lzms']:
            if layout == 'solid' and codec == 'none':
                continue
            wim = root / f'{layout}-{codec}.wim'
            options = ['--compress=' + codec] if layout != 'solid' else ['--solid', '--solid-compress=' + codec]
            if layout == 'pipable':
                options += ['--pipable']
            subprocess.run([str(imagex), 'capture', str(tree), str(wim), 'First', *options, '--threads=1'], check=True, stdout=subprocess.DEVNULL)
            if layout != 'pipable':
                subprocess.run([str(imagex), 'append', str(tree), str(wim), 'Second', '--threads=1'], check=True, stdout=subprocess.DEVNULL)
            outputs = [subprocess.check_output([str(client), str(wim)], text=True).splitlines() for client in clients]
            # Content hash-table traversal order is unspecified. Metadata still
            # appears first, in image order, and is checked without sorting.
            rows = [[line for line in output if line.startswith('row:')] for output in outputs]
            metadata = [[line for line in values if line.split(':')[8] == '1'] for values in rows]
            for values, image_rows in zip(rows, metadata):
                assert values[:len(image_rows)] == image_rows, (layout, codec, 'metadata must precede content')
            assert metadata[0] == metadata[1], (layout, codec, metadata)
            normalized = [sorted(output) for output in outputs]
            assert normalized[0] == normalized[1], (layout, codec, outputs)
            results.append({'layout': layout, 'codec': codec, 'rows': len(rows[0]), 'metadata_rows': len(metadata[0]), 'normalized_sha256': hashlib.sha256('\n'.join(normalized[0]).encode()).hexdigest()})
print(json.dumps({'passed': True, 'cases': len(results), 'results': results}, indent=2))
