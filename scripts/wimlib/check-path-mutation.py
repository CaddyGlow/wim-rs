#!/usr/bin/env python3
"""Original/native path mutation contracts, including unchanged trees on errors."""
import argparse
import hashlib
import json
import os
import stat
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=Path('/tmp/wimlib'))
p.add_argument('--oracle', type=Path, default=Path('/tmp/wimlib-native-oracle'))
p.add_argument('--native', type=Path)
p.add_argument('--write-interop', action='store_true')
p.add_argument('--diagnostics', action='store_true')
p.add_argument('--layout', choices=['ordinary', 'solid', 'pipable'], default='ordinary')
p.add_argument('--codec', choices=['none', 'xpress', 'lzx', 'lzms'], default='none')
p.add_argument('--unicode-names', action='store_true')
p.add_argument('--ignore-case', action='store_true')
a = p.parse_args()
results = []
def snapshot(directory):
    entries = []
    groups = {}
    for parent, dirs, files in os.walk(directory):
        for name in sorted(dirs + files):
            node = Path(parent) / name
            relative = str(node.relative_to(directory))
            info = node.lstat()
            record = {'path': relative, 'mode': info.st_mode}
            if stat.S_ISREG(info.st_mode):
                record['sha256'] = hashlib.sha256(node.read_bytes()).hexdigest()
                groups.setdefault((info.st_dev, info.st_ino), []).append(relative)
            elif stat.S_ISLNK(info.st_mode):
                record['target'] = os.readlink(node)
            record['xattrs'] = {key: os.getxattr(node, key, follow_symlinks=False).hex() for key in sorted(os.listxattr(node, follow_symlinks=False))}
            entries.append(record)
    return {'entries': sorted(entries, key=lambda e: e['path']), 'hardlinks': sorted(sorted(group) for group in groups.values())}
with tempfile.TemporaryDirectory(prefix='wim-path-mutation-') as directory:
    root = Path(directory)
    clients = []
    libraries = [('original', a.oracle / '.libs')]
    if a.native is not None:
        libraries.append(('native', a.native))
    for name, library in libraries:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-path-mutation.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    tree = root / 'tree'
    tree.mkdir()
    for name in ['dir', 'empty', 'other']:
        (tree / name).mkdir()
    (tree / 'file').write_bytes(b'mutable resource\n' * 128)
    (tree / 'other-file').write_bytes(b'replacement')
    (tree / 'dir' / 'child').write_bytes(b'nested')
    os.link(tree / 'file', tree / 'alias')
    (tree / 'symlink').symlink_to('file')
    if a.unicode_names:
        for name in ['Unicode 😀', 'école', 'Σ', 'σ', 'ς']:
            (tree / name).write_bytes(name.encode())
    path = root / 'source.wim'
    environment = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
    if a.diagnostics: environment['WIM_MUTATION_DIAGNOSTICS'] = '1'
    if a.ignore_case: environment['WIM_MUTATION_IGNORE_CASE'] = '1'
    if a.layout == 'solid' and a.codec == 'none':
        p.error('solid layout requires a compressed codec')
    options = ['--compress=' + a.codec] if a.layout != 'solid' else ['--solid', '--solid-compress=' + a.codec]
    if a.layout == 'pipable': options.append('--pipable')
    subprocess.run([str(a.oracle / 'wimlib-imagex'), 'capture', str(tree), str(path), 'Mutation', *options, '--unix-data', '--threads=1'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=environment, check=True)
    cases = []
    for image in [-1, 0, 1, 2]:
        for source in ['file', '/file', r'\dir\child', 'dir', 'empty', '/', '', '@NULL', 'missing', 'file/child', 'dir/../file', 'alias']:
            for flags in [0, 1, 2, 3, 4, -1]:
                cases.append(['delete', str(image), source, '', str(flags), 'single'])
    for source in ['file', 'dir', 'empty', '/', 'missing', 'alias', '@NULL']:
        for destination in ['new', 'other-file', 'file', 'dir', 'empty', 'dir/child', 'dir/new', 'missing/new', 'file/new', '/', '', '@NULL', r'\other\renamed']:
            cases.append(['rename', '1', source, destination, '0', 'single'])
    for op in ['delete', 'rename']:
        for ownership in ['shared', 'dest-shared', 'dest-released', 'source-released']:
            cases.append([op, '1', 'file', 'new', '0', ownership])
    if a.unicode_names:
        for source in ['Unicode 😀', 'unicode 😀', 'école', 'ÉCOLE', 'Σ', 'σ', 'ς', 'FILE', 'DIR/CHILD']:
            cases.append(['delete', '1', source, '', '0', 'single'])
            for target in ['renamed 😀', 'Σ', 'σ', 'ς']:
                cases.append(['rename', '1', source, target, '0', 'single'])
    written = 0
    applied_pairs = 0
    for index, arguments in enumerate(cases):
        completed = [subprocess.run([str(client), str(path), *arguments], env=environment, capture_output=True, check=True) for client in clients]
        outputs = [r.stdout for r in completed]
        if len(outputs) == 2:
            assert outputs[0] == outputs[1], (arguments, outputs)
            assert completed[0].stderr == completed[1].stderr, (arguments, [r.stderr for r in completed])
        results.append({'arguments': arguments, 'stdout': outputs[0].decode(), 'stderr': completed[0].stderr.decode(), 'stdout_sha256': hashlib.sha256(outputs[0]).hexdigest()})
        if a.write_interop and 'result:0:errno:' in outputs[0].decode():
            trees = []
            post_write = []
            for label, client in zip(['original', 'native'], clients):
                output = root / f'written-{index}-{label}.wim'
                result = subprocess.check_output([str(client), str(path), *arguments, str(output)], env=environment)
                assert b'write:0\n' in result, (arguments, label, result)
                post_write.append(result)
                subprocess.run([str(a.oracle / 'wimlib-imagex'), 'verify', str(output)], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, check=True)
                written += 1
                applied = root / f'applied-{index}-{label}'
                subprocess.run([str(a.oracle / 'wimlib-imagex'), 'apply', str(output), '1', str(applied), '--unix-data'], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, check=True)
                trees.append(snapshot(applied))
            if len(trees) == 2:
                assert post_write[0] == post_write[1], (arguments, post_write)
                assert trees[0] == trees[1], (arguments, trees)
                applied_pairs += 1
print(json.dumps({'scope': 'original contract' if a.native is None else 'original/native differential', 'native_sha256': hashlib.sha256((a.native / 'libwim.so').read_bytes()).hexdigest() if a.native else None, 'layout': a.layout, 'codec': a.codec, 'unicode_names': a.unicode_names, 'ignore_case': a.ignore_case, 'cases': len(results), 'original_verified_written_archives': written, 'equal_original_applied_pairs': applied_pairs, 'original_capture_disabled_cpu_feature': 'sse4.2', 'results': results}, indent=2))
