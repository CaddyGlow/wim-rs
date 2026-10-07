#!/usr/bin/env python3
"""Compare capture export ownership after source release and original-reader output."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile

def snapshot(directory):
    rows = []
    groups = {}
    for path in sorted(directory.rglob('*')):
        info = path.lstat()
        relative = str(path.relative_to(directory))
        if stat.S_ISLNK(info.st_mode):
            data = ['link', os.readlink(path)]
        elif stat.S_ISDIR(info.st_mode):
            data = ['directory']
        else:
            data = ['file', hashlib.sha256(path.read_bytes()).hexdigest()]
            groups.setdefault((info.st_dev, info.st_ino), []).append(relative)
        rows.append([relative, stat.S_IMODE(info.st_mode), data])
    return {'entries': rows, 'hardlinks': sorted(sorted(group) for group in groups.values())}

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
rows = []
with tempfile.TemporaryDirectory(prefix='wim-capture-export-') as directory:
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
    env = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
    oracle = '/tmp/wimlib-native-oracle/wimlib-imagex'
    for phase, flags, kind in itertools.product([0, 1], [0, 2, 4, 8], ['ordinary', 'hardlinks', 'symlink']):
        tree = root / f'tree-{phase}-{flags}-{kind}'
        tree.mkdir()
        (tree / 'file').write_bytes(b'capture export' * 2500)
        if kind == 'hardlinks':
            os.link(tree / 'file', tree / 'alias')
        elif kind == 'symlink':
            os.symlink('file', tree / 'link')
        before = snapshot(tree)
        outputs = []
        states = []
        verified = []
        for index, client in enumerate(clients):
            output = root / f'output-{phase}-{flags}-{kind}-{index}.wim'
            outputs.append(subprocess.check_output([str(client), str(tree), str(output), str(phase), str(flags)], env=env).decode())
            if outputs[-1].splitlines()[-1] == 'write 0':
                verification = subprocess.run([oracle, 'verify', str(output)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env)
                verified.append(verification.returncode)
                if verification.returncode:
                    states.append(None)
                    continue
                target = root / f'apply-{phase}-{flags}-{kind}-{index}'
                subprocess.run([oracle, 'apply', str(output), '1', str(target)], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env)
                states.append(snapshot(target))
            else:
                states.append(None)
                verified.append(None)
        assert snapshot(tree) == before
        rows.append({'phase': phase, 'flags': flags, 'kind': kind, 'original': outputs[0], 'native': outputs[1],
                     'equal': outputs[0] == outputs[1] and states[0] == states[1] and verified[0] == verified[1], 'original_reader_status': verified, 'apply': states})
print(json.dumps({'cases': len(rows), 'equal': sum(row['equal'] for row in rows), 'native_sha256': digest, 'results': rows}, indent=2))
raise SystemExit(any(not row['equal'] for row in rows))
