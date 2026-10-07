#!/usr/bin/env python3
"""Measure real Windows token privileges around library initialization/cleanup."""
import argparse
import hashlib
import json
import pathlib
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-capture/privileges-original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-privileges-20261003' + '\\' + args.implementation
    assert guest.powershell("New-Item -ItemType Directory -Force '" + root + "' | Out-Null")['exit'] == 0
    dll = args.dll.read_bytes()
    caller = pathlib.Path('target/windows-abi-probe/probe-windows-privileges.exe').read_bytes()
    guest.put(root + r'\wim.dll', dll)
    guest.put(root + r'\probe.exe', caller)
    observations = []
    for flags, repeated in [(flags, 0) for flags in [0, 2, 4, 8, 12, 6, 10, 14]] + [(0, 1), (2, 1)]:
        result = guest.execute(root + r'\probe.exe', [root + r'\wim.dll', str(flags), str(repeated)])
        observations.append({'flags': flags, 'repeated': repeated, 'result': result})
    result = {'scope': 'Actual SYSTEM process-token flags; all requested rights assigned; restricted-token failures remain gated',
              'privilege_order': ['Backup', 'Security', 'Restore', 'TakeOwnership', 'ManageVolume'],
              'implementation': args.implementation, 'dll_sha256': hashlib.sha256(dll).hexdigest(),
              'caller_sha256': hashlib.sha256(caller).hexdigest(), 'cases': observations}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        result['differences'] = [{'flags': a['flags'], 'repeated': a['repeated'], 'original': a['result'], 'native': b['result']}
                                 for a, b in zip(baseline['cases'], observations) if a['result'] != b['result']]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'exits': [entry['result']['exit'] for entry in observations],
                      'differences': len(result.get('differences', []))}, indent=2))


if __name__ == '__main__':
    main()
