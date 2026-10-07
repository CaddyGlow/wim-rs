#!/usr/bin/env python3
"""Prepare copies of full ObjectID challenges without source mutation."""
import argparse
import hashlib
import json
from pathlib import Path


def extended_challenge(encoded):
    raw = bytes.fromhex(encoded)
    if len(raw) != 64 or not any(raw[:16]) or not any(raw[16:48]):
        raise ValueError('requires full64 ObjectID with nonzero original16 and birth32')
    return raw[16:]


def prepare(path):
    data = path.read_bytes()
    challenges = {}
    for entry in json.loads(data)['entries']:
        if entry['ObjectIdRaw']:
            extension = extended_challenge(entry['ObjectIdRaw'])
            challenges.setdefault(extension.hex(), []).append(entry['Path'])
    if not challenges:
        raise ValueError('no nonzero extended challenge')
    return {'schema': 1, 'executed': False, 'fixture_sha256': hashlib.sha256(data).hexdigest(),
            'challenges': [{'extended48_hex': raw, 'source_paths_provenance_only': paths}
                           for raw, paths in challenges.items()]}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('fixture', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = prepare(args.fixture)
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
