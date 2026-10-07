#!/usr/bin/env python3
"""Compare original C stdout header/image output, including mutable platform XML."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--source', type=Path, default=Path('/tmp/wimlib'))
p.add_argument('--oracle', type=Path, default=Path('/tmp/wimlib-native-oracle'))
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
results = []
with tempfile.TemporaryDirectory(prefix='wim-print-') as directory:
    root = Path(directory)
    clients = []
    for name, library in [('original', a.oracle / '.libs'), ('native', a.native)]:
        client = root / name
        subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I' + str((a.source / 'include').resolve()), 'scripts/wimlib/probe-print-api.c', '-L' + str(library.resolve()), '-Wl,-rpath,' + str(library.resolve()), '-lwim', '-o', str(client)], check=True)
        clients.append(client)
    tree = root / 'tree'
    tree.mkdir()
    (tree / 'data').write_bytes(b'original information printing\n' * 2048)
    fixtures = [(str(root / f'new-{codec}'), 'new', codec) for codec in range(4)]
    fixtures.append(('pending', 'empty', 0))
    environment = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')
    for layout in ['ordinary', 'solid', 'pipable']:
        for codec in ['none', 'xpress', 'lzx', 'lzms']:
            if layout == 'solid' and codec == 'none': continue
            path = root / f'{layout}-{codec}.wim'
            options = ['--compress=' + codec] if layout != 'solid' else ['--solid', '--solid-compress=' + codec]
            if layout == 'pipable': options += ['--pipable']
            subprocess.run([str(a.oracle / 'wimlib-imagex'), 'capture', str(tree), str(path), 'Original 😀', *options, '--check', '--threads=1'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=environment, check=True)
            fixtures.append((path.name, str(path), 0))
    fixed_times = ['CREATIONTIME/HIGHPART', '0', 'CREATIONTIME/LOWPART', '0', 'LASTMODIFICATIONTIME/HIGHPART', '0', 'LASTMODIFICATIONTIME/LOWPART', '0']
    for label, path, codec in fixtures:
        for selected in [-2, -1, 0, 1, 2, 2147483647]:
            arguments = [path, str(codec), str(selected), *fixed_times]
            outputs = [subprocess.check_output([str(client), *arguments]) for client in clients]
            assert outputs[0] == outputs[1], (label, selected, outputs)
            results.append({'fixture': label, 'image': selected, 'stdout_sha256': hashlib.sha256(outputs[0]).hexdigest()})
    base = str(root / 'ordinary-none.wim')
    sets = [
        ['WINDOWS/ARCH', str(arch)] for arch in range(18)
    ]
    sets += [
        ['NAME', b'raw\xffname', 'DESCRIPTION', b'\xed\xa0\x80', 'DISPLAYNAME', 'Visible', 'DISPLAYDESCRIPTION', 'Display description'],
        ['WINDOWS/PRODUCTNAME', 'Windows Test', 'WINDOWS/EDITIONID', 'Professional', 'WINDOWS/INSTALLATIONTYPE', 'Client', 'WINDOWS/HAL', 'HAL 😀', 'WINDOWS/PRODUCTTYPE', 'WinNT', 'WINDOWS/PRODUCTSUITE', 'Suite', 'WINDOWS/SYSTEMROOT', 'WINDOWS', 'WINDOWS/LANGUAGES/LANGUAGE[1]', 'en-US', 'WINDOWS/LANGUAGES/LANGUAGE[2]', 'fr-FR', 'WINDOWS/LANGUAGES/DEFAULT', 'en-US', 'WINDOWS/VERSION/MAJOR', '10', 'WINDOWS/VERSION/MINOR', '0', 'WINDOWS/VERSION/BUILD', '26100', 'WINDOWS/VERSION/SPBUILD', '1', 'WINDOWS/VERSION/SPLEVEL', '2', 'FLAGS', 'Professional', 'WIMBOOT', '1'],
        ['WINDOWS/VERSION/MAJOR', '10', 'WINDOWS/VERSION/MAJOR', '', 'WINDOWS/LANGUAGES/LANGUAGE', 'en-US', 'WINDOWS/LANGUAGES/LANGUAGE', ''],
        ['CREATIONTIME/HIGHPART[2]', '1', 'CREATIONTIME/LOWPART[2]', '1'],
    ]
    for value in ['0', '42', '-2', '-1', '+17', ' 19', '19 ', '0x10', '18446744073709551614', '18446744073709551615', '18446744073709551616', 'garbage']:
        sets.append(['DIRCOUNT', value, 'FILECOUNT', value, 'TOTALBYTES', value, 'HARDLINKBYTES', value, 'WINDOWS/ARCH', value, 'WIMBOOT', value])
    for value in ['0', 'ffffffff', '0x01d00000', '-2', 'ffffffffffffffff', '10000000000000000', 'wrong']:
        sets.append(['CREATIONTIME/HIGHPART', value, 'CREATIONTIME/LOWPART', value])
    for index, properties in enumerate(sets):
        outputs = [subprocess.check_output([str(client), base, '0', '-1', *fixed_times, *properties]) for client in clients]
        assert outputs[0] == outputs[1], (index, properties, outputs)
        results.append({'properties': index, 'stdout_sha256': hashlib.sha256(outputs[0]).hexdigest()})
print(json.dumps({'equal': True, 'cases': len(results), 'original_capture_disabled_cpu_feature': 'sse4.2', 'results': results}, indent=2))
