#!/usr/bin/env python3
"""Compare unchanged-header template-image calls; preserve reference before native runs."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--native', type=Path, default=Path('target/debug/libwim.so'))
parser.add_argument('--output', type=Path, default=Path('docs/wimlib/evidence/native-ffi-template/differential.json'))
args = parser.parse_args()
environment = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
with tempfile.TemporaryDirectory(prefix='wim-template-api-') as directory:
    root = Path(directory)
    source = root / 'source'
    source.mkdir()
    (source / 'file').write_bytes(bytes(range(256)) * 300 + b'last chunk')
    os.link(source / 'file', source / 'alias')
    template = root / 'template.wim'
    subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex', 'capture', str(source),
                    str(template), '--compress=none', '--no-acls', '--nocheck'],
                   check=True, capture_output=True, env=environment)
    clients = {}
    hashes = {}
    errors = {}
    for name, library in [('original', Path('/tmp/wimlib-native-oracle/.libs/libwim.so')),
                          ('native', args.native)]:
        folder = root / name
        folder.mkdir()
        shutil.copyfile(library.resolve(), folder / 'libwim.so')
        (folder / 'libwim.so.15').symlink_to('libwim.so')
        hashes[name] = hashlib.sha256((folder / 'libwim.so').read_bytes()).hexdigest()
        client = folder / 'client'
        build = subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                                'scripts/wimlib/probe-template-image.c', '-L' + str(folder),
                                '-Wl,-rpath,' + str(folder), '-lwim', '-o', str(client)],
                               capture_output=True, text=True)
        if build.returncode:
            errors[name] = build.stderr
        else:
            clients[name] = client
    rows = []
    for kind, new_image, template_image, flags, mode in itertools.product(
            ['empty', 'clean', 'capture'], [-1, 0, 1, 2], [-1, 0, 1, 2], [0, 1, -1], range(4)):
        case = dict(kind=kind, new_image=new_image, template_image=template_image,
                    flags=flags, mode=mode)
        row = dict(case=case)
        for name in ['original', 'native']:
            if name not in clients:
                continue
            result = subprocess.run([str(clients[name]), str(template),
                                     str(source) if kind == 'capture' else kind,
                                     str(new_image), str(template_image), str(flags), str(mode), 'skip'],
                                    capture_output=True, text=True, env=environment, check=True)
            row[name] = result.stdout
        rows.append(row)
    common = dict(source_commit='cd5e231c348c255ae5088873b5a66ee0eb96fa07',
                  library_sha256=hashes, cases=len(rows))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    reference = args.output.with_name('original-contracts.json')
    reference.write_text(json.dumps(dict(common, scope='original reference only',
                                         results=[dict(case=r['case'], original=r['original']) for r in rows]), indent=2) + '\n')
    exact = sum(row.get('native') == row['original'] for row in rows)
    result = dict(common, exact=exact, link_errors=errors, results=rows)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(common, exact=exact, link_errors=errors), indent=2))
