#!/usr/bin/env python3
"""Independently verify reconstructed WIMs and export the generated ISO for boot testing."""
import argparse
import base64
import hashlib
import json
from pathlib import Path

from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--run-json', type=Path, required=True)
    parser.add_argument('--iso', type=Path, required=True)
    parser.add_argument('--oracle', default=r'D:\tools\wimlib-imagex.exe')
    args = parser.parse_args()
    run = json.loads(args.run_json.read_text())
    if not run.get('source_payloads_preserved') or any(s['exit'] != 0 for s in run['steps']):
        raise RuntimeError('Requires successful reconstruction, servicing and source preservation evidence')
    if args.iso.exists():
        raise RuntimeError('Choose a new ISO output; existing artifacts are preserved')
    guest = WindowsGuest(args.qga_socket)
    root = run['guest_root']
    media = root + r'\assembly\base-media'
    evidence = {'pipeline': str(args.run_json), 'steps': [], 'iso': str(args.iso)}
    destination = args.run_json.parent / 'media-verification.json'

    def save():
        destination.write_text(json.dumps(evidence, indent=2) + '\n')

    for name, path in [('boot', media + r'\sources\boot.wim'),
                       ('install', media + r'\sources\install.wim'),
                       ('serviced', run.get('servicing_directory', root + r'\servicing') + r'\install.wim')]:
        for command in ['verify', 'info']:
            result = guest.execute(args.oracle, [command, path], timeout=1800)
            result.pop('stdout_base64', None)
            result.update(image=name, command=command, executable=args.oracle, path=path)
            evidence['steps'].append(result)
            save()
            print(json.dumps({'image': name, 'command': command, 'exit': result['exit']}), flush=True)
            if result['exit'] != 0:
                return 1
    remote = root + r'\reconstructed.iso'
    result = guest.execute(run.get('cli_path', root + r'\windows-uup.exe'), ['iso', '--source', media, '--output', remote], timeout=1800)
    result.pop('stdout_base64', None)
    result.update(command='iso', path=remote)
    evidence['steps'].append(result)
    save()
    if result['exit'] != 0:
        return 1
    expected = json.loads(result['stdout'])['sha256']
    args.iso.parent.mkdir(parents=True, exist_ok=True)
    handle = guest.call('guest-file-open', {'path': remote, 'mode': 'rb'})
    digest = hashlib.sha256()
    try:
        with args.iso.open('xb') as output:
            while True:
                block = guest.call('guest-file-read', {'handle': handle, 'count': 65536})
                data = base64.b64decode(block.get('buf-b64', ''))
                output.write(data)
                digest.update(data)
                if block['eof']:
                    break
    finally:
        guest.call('guest-file-close', {'handle': handle})
    evidence.update(iso_sha256=digest.hexdigest(), iso_bytes=args.iso.stat().st_size,
                    transfer_hash_matches=digest.hexdigest().lower() == expected.lower())
    save()
    if not evidence['transfer_hash_matches']:
        raise RuntimeError('Transferred ISO hash differs from production writer result')
    print(json.dumps({'iso': str(args.iso), 'sha256': digest.hexdigest()}), flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
