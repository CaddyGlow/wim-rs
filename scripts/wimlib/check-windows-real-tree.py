#!/usr/bin/env python3
"""Compare complete Windows-directory metadata from a retained frozen fixture."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import time
import zipfile

from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--fixture-evidence', required=True, type=Path)
    parser.add_argument('--dll', required=True, type=Path)
    parser.add_argument('--probe', required=True, type=Path,
                        help='capture C probe built with CAPTURE_BUFFERED_OUTPUT')
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    fixture = json.loads(args.fixture_evidence.read_text())
    guest = WindowsGuest(args.qga_socket)
    root = fixture['fixture']
    source = fixture['snapshot']['DeviceObject'] + r'\Windows'
    evidence = {'source': source, 'snapshot': fixture['snapshot'],
                'environment': fixture['setup'], 'cases': [],
                'artifacts': {'original.dll': fixture['artifacts']['original.dll']}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for name, path in [('rust-tree.dll', args.dll), ('tree.exe', args.probe)]:
        data = path.read_bytes()
        guest.put(root + '\\' + name, data)
        evidence['artifacts'][name] = hashlib.sha256(data).hexdigest()
    inventories = []
    for label in ['original', 'rust']:
        dll = 'original.dll' if label == 'original' else 'rust-tree.dll'
        log = root + '\\tree-' + label + '.txt'
        archive = root + '\\tree-' + label + '-' + str(time.time_ns()) + '.zip'
        command = ("$ErrorActionPreference='Stop';& '" + root + "\\tree.exe' '" + root + '\\' + dll +
                   "' '" + source + "' '-' '64' '-' '-1' '0' '12' > '" + log +
                   "';$rc=$LASTEXITCODE;Compress-Archive -LiteralPath '" + log + "' -DestinationPath '" +
                   archive + "';Get-Content -LiteralPath '" + log + "' -Tail 1;exit $rc")
        started = time.monotonic()
        result = guest.execute(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
                               ['-NoProfile', '-NonInteractive', '-Command', command], timeout=1800)
        result.pop('stdout_base64', None)
        with zipfile.ZipFile(io.BytesIO(guest.get(archive))) as zipped:
            if len(zipped.namelist()) != 1:
                raise RuntimeError('unexpected manifest archive members')
            lines = zipped.read(zipped.namelist()[0]).decode('utf-16').splitlines()
        nodes = {}
        current = None
        for line in lines:
            if line.startswith('node '):
                current = line.split()[1]
                if current in nodes:
                    raise RuntimeError('duplicate path in manifest')
                nodes[current] = [line]
            elif current is not None and line.startswith(('dos ', 'sd ', 'stream ')):
                nodes[current].append(line)
        passes = (result['exit'] == 0 and not result.get('stdout_truncated') and
                  'init 0' in lines and 'add 0' in lines and 'tree 0' in lines and bool(nodes))
        case = {'label': label, 'result': result, 'passes': passes,
                'elapsed_seconds': round(time.monotonic() - started, 2), 'nodes': len(nodes),
                'manifest_sha256': hashlib.sha256(json.dumps(nodes, sort_keys=True).encode()).hexdigest()}
        evidence['cases'].append(case)
        args.output.write_text(json.dumps(evidence, indent=2) + '\n')
        print(json.dumps(case), flush=True)
        inventories.append(nodes)
    original, rust = inventories
    missing = sorted(original.keys() - rust.keys())
    extra = sorted(rust.keys() - original.keys())
    differences = [{'path': path, 'original': original[path], 'rust': rust[path]}
                   for path in sorted(original.keys() & rust.keys()) if original[path] != rust[path]]
    evidence.update({'missing_count': len(missing), 'extra_count': len(extra),
                     'difference_count': len(differences), 'missing': missing[:100],
                     'extra': extra[:100], 'differences': differences[:100],
                     'passes': all(case['passes'] for case in evidence['cases']) and original == rust})
    args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps({'passes': evidence['passes'], 'nodes': len(rust), 'differences': len(differences)}))
    return 0 if evidence['passes'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
