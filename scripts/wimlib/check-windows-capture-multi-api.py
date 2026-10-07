#!/usr/bin/env python3
"""Run real same-inode capture controls across update-command boundaries."""
import argparse
import hashlib
import json
import pathlib
import struct
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-capture/multi-original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-capture-multi-run-20261003' + '\\' + args.implementation
    assert guest.powershell("New-Item -ItemType Directory -Force '" + root + "' | Out-Null")['exit'] == 0
    dll = args.dll.read_bytes()
    caller = pathlib.Path('/tmp/probe-windows-capture-multi.exe').read_bytes()
    guest.put(root + r'\wim.dll', dll)
    guest.put(root + r'\probe.exe', caller)
    observations = []
    for scenario in range(5):
        output = root + '\\multi-' + str(scenario) + '.wim' if args.write else '-'
        result = guest.execute(root + r'\probe.exe', [root + r'\wim.dll', str(scenario), output])
        record = {'scenario': scenario, 'result': result}
        if args.write and 'write 0' in result['stdout']:
            data = guest.get(output)
            lookup_size = struct.unpack_from('<Q', data, 48)[0] & ((1 << 56) - 1)
            lookup_offset = struct.unpack_from('<Q', data, 56)[0]
            for offset in range(lookup_offset, lookup_offset + lookup_size, 50):
                stored = struct.unpack_from('<Q', data, offset)[0]
                if stored >> 56 & 2:
                    assert not stored >> 56 & 4
                    resource_offset = struct.unpack_from('<Q', data, offset + 8)[0]
                    metadata = data[resource_offset:resource_offset + (stored & ((1 << 56) - 1))]
                    security_size, security_count = struct.unpack_from('<II', metadata)
                    root_offset = (security_size + 7) & ~7
                    node_ids = [{'name': '', 'security_id': struct.unpack_from('<I', metadata, root_offset + 12)[0]}]
                    inode_groups = []
                    child = struct.unpack_from('<Q', metadata, root_offset + 16)[0]
                    while child and child < len(metadata):
                        length = struct.unpack_from('<Q', metadata, child)[0]
                        if not length:
                            break
                        name_length = struct.unpack_from('<H', metadata, child + 100)[0]
                        node_ids.append({'name': metadata[child + 102:child + 102 + name_length].decode('utf-16le'),
                                         'security_id': struct.unpack_from('<I', metadata, child + 12)[0]})
                        inode_groups.append((node_ids[-1]['name'], struct.unpack_from('<Q', metadata, child + 88)[0]))
                        child += length
                    record['metadata'] = {'size': len(metadata), 'security_count': security_count,
                                          'security_table': metadata[:security_size].hex(),
                                          'node_security_ids': node_ids,
                                          'hardlink_pairs': [[left, right] for i, (left, group) in enumerate(inode_groups)
                                                             for right, other in inode_groups[i + 1:] if group and group == other],
                                          'sha1_valid': hashlib.sha1(metadata).digest() == data[offset + 30:offset + 50]}
                    break
        observations.append(record)
    result = {'scope': 'Source-first actual Windows same-call versus separate-call hardlink security metadata',
              'implementation': args.implementation, 'dll_sha256': hashlib.sha256(dll).hexdigest(),
              'caller_sha256': hashlib.sha256(caller).hexdigest(), 'cases': observations}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        result['differences'] = [{'scenario': a['scenario'], 'original': a['result'], 'native': b['result']}
                                 for a, b in zip(baseline['cases'], observations)
                                 if a['result'] != b['result'] or a.get('metadata') != b.get('metadata')]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'exits': [entry['result']['exit'] for entry in observations],
                      'differences': len(result.get('differences', []))}, indent=2))


if __name__ == '__main__':
    main()
