#!/usr/bin/env python3
"""Export bounded WIM metadata ranges from an owned QGA guest.

The sparse output is NOT a complete WIM and must never be installed or claimed
fully verified. Retain the complete guest archive and its independently recorded
full SHA256/integrity verification. libwim verifies metadata SHA1 while decoding.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path

from windows_guest import WindowsGuest

spec = importlib.util.spec_from_file_location('layout', Path(__file__).with_name('check-qcow2-reparse-layout.py'))
layout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout)


class MetadataSnapshot(layout.Archive):
    def __init__(self, path, library, guest, guest_handle, length, budget):
        self.guest = guest
        self.guest_handle = guest_handle
        self.transfer_budget = budget
        self.transferred = 0
        self.ranges = []
        self.destination = path.open('xb')
        self.destination.truncate(length)
        self.cache = {}
        try:
            super().__init__(path, library)
        except BaseException:
            self.destination.close()
            raise

    def read(self, offset, size):
        if size > layout.LIMIT or offset < 0 or offset + size > self.length:
            raise ValueError('range exceeds complete guest archive bounds')
        if (offset, size) in self.cache:
            return self.cache[(offset, size)]
        if self.transferred + size > self.transfer_budget:
            raise ValueError('encoded metadata transfer exceeds explicit budget')
        self.guest.call('guest-file-seek', {'handle': self.guest_handle, 'offset': offset, 'whence': 0})
        chunks = []
        remaining = size
        while remaining:
            reply = self.guest.call('guest-file-read', {'handle': self.guest_handle, 'count': min(65536, remaining)})
            chunk = base64.b64decode(reply['buf-b64'], validate=True)
            if len(chunk) != reply['count'] or not chunk or len(chunk) > remaining:
                raise ValueError('short or oversized guest metadata read')
            chunks.append(chunk)
            remaining -= len(chunk)
        data = b''.join(chunks)
        self.destination.seek(offset)
        self.destination.write(data)
        self.destination.flush()
        self.transferred += size
        self.ranges.append({'offset': offset, 'length': size, 'sha256': hashlib.sha256(data).hexdigest()})
        self.cache[(offset, size)] = data
        return data

    def close(self):
        super().close()
        self.destination.flush()
        os.fsync(self.destination.fileno())
        self.destination.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga', type=Path, required=True)
    parser.add_argument('--guest-wim', required=True)
    parser.add_argument('--size', type=int, required=True)
    parser.add_argument('--full-sha256', required=True)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--budget', type=int, default=64 * 1024 * 1024)
    args = parser.parse_args()
    if not 208 <= args.size <= 64 * 1024**3 or not 208 <= args.budget <= 256 * 1024**2:
        raise ValueError('invalid complete length or transfer budget')
    if len(args.full_sha256) != 64 or bytes.fromhex(args.full_sha256).hex() != args.full_sha256:
        raise ValueError('invalid recorded full archive SHA256')
    guest = WindowsGuest(args.qga)
    handle = guest.call('guest-file-open', {'path': args.guest_wim, 'mode': 'r'})
    archive = None
    try:
        length = guest.call('guest-file-seek', {'handle': handle, 'offset': 0, 'whence': 2})['position']
        if length != args.size:
            raise ValueError('guest archive length changed')
        archive = MetadataSnapshot(args.output, args.library, guest, handle, length, args.budget)
        archive.resource(layout.descriptor(archive.read(72, 24)))
        report = {'scope': 'METADATA ONLY: sparse snapshot omits file payload ranges; not a valid complete installation archive',
                  'guest_archive': args.guest_wim, 'complete_guest_bytes': length,
                  'recorded_complete_sha256': args.full_sha256,
                  'complete_sha256_scope': 'Caller-supplied independent full guest archive provenance; not recomputed by bounded transfer',
                  'metadata_sha1_verified_by_independent_libwim': hashlib.sha1(archive.metadata).hexdigest(),
                  'encoded_bytes_transferred': archive.transferred, 'encoded_budget': args.budget,
                  'ranges': archive.ranges}
        args.report.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({key: value for key, value in report.items() if key != 'ranges'}))
    finally:
        if archive is not None:
            archive.close()
        guest.call('guest-file-close', {'handle': handle})


if __name__ == '__main__':
    main()
