#!/usr/bin/env python3
"""Stream a completed guest file through bounded QGA RPCs and verify its identity."""
import argparse
import base64
import hashlib
import os
from pathlib import Path
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--guest-path', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--expected-size', required=True, type=int)
    parser.add_argument('--expected-sha256', required=True)
    args = parser.parse_args()
    if not 0 <= args.expected_size <= 64 * 1024**3:
        raise ValueError('expected file size exceeds bounded transfer profile')
    expected = bytes.fromhex(args.expected_sha256)
    if len(expected) != 32:
        raise ValueError('requires SHA256 identity')
    guest = WindowsGuest(args.qga_socket)
    handle = guest.call('guest-file-open', {'path': args.guest_path, 'mode': 'rb'})
    digest, total = hashlib.sha256(), 0
    temporary = args.output.with_name(args.output.name + '.partial')
    try:
        if args.output.exists():
            raise FileExistsError(args.output)
        with temporary.open('xb') as destination:
            while True:
                result = guest.call('guest-file-read', {'handle': handle, 'count': 65536})
                chunk = base64.b64decode(result.get('buf-b64', ''), validate=True)
                if len(chunk) != result['count'] or total + len(chunk) > args.expected_size:
                    raise ValueError('guest file exceeded expected identity bounds')
                destination.write(chunk)
                digest.update(chunk)
                total += len(chunk)
                if result['eof']:
                    break
                if not chunk:
                    raise ValueError('guest file read made no progress')
            destination.flush()
            os.fsync(destination.fileno())
        if total != args.expected_size or digest.digest() != expected:
            raise ValueError('guest file size/hash identity mismatch; partial retained')
        # Exclusive linking prevents replacing a destination created during transfer.
        os.link(temporary, args.output)
        temporary.unlink()
        print(f'{total} bytes SHA256 {digest.hexdigest()}')
    finally:
        guest.call('guest-file-close', {'handle': handle})


if __name__ == '__main__':
    main()
