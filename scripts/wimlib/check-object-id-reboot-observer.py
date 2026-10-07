#!/usr/bin/env python3
"""Prepare/validate GET-only observations of the existing frozen scratch challenge."""
import argparse
import hashlib
import json
from pathlib import Path


SCRATCH_ROOT = r'T:\objectid-scratch-8e13fcf95dd746739b3ba4f1b14a64af'


def validate_selector(encoded):
    raw = bytes.fromhex(encoded)
    if not raw or len(raw) % 2:
        raise ValueError('invalid raw UTF16 selector')
    path = raw.decode('utf-16-le', errors='surrogatepass')
    if '\0' in path or '/' in path or not path.startswith(SCRATCH_ROOT + '\\') or any(part in ('', '.', '..') for part in path[3:].split('\\')):
        raise ValueError('selector escapes exact existing scratch tree')
    return path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate_row(expected, actual):
    validate_selector(expected['path_utf16_le'])
    validate_selector(actual['path_utf16_le'])
    if actual['path_utf16_le'] != expected['path_utf16_le']:
        raise ValueError('changed raw UTF16 selector')
    if actual.get('error'):
        raise ValueError('scratch selector could not be observed: ' + actual['error'])
    for key, size in [('object_id_full64', 64), ('file_id_info24', 24)]:
        try:
            value = bytes.fromhex(actual[key])
            original = bytes.fromhex(expected[key])
        except (TypeError, ValueError, KeyError) as error:
            raise ValueError('invalid observation bytes') from error
        if len(value) != size or len(original) != size or value != original:
            raise ValueError('changed or incomplete ' + key)
    if actual.get('passed') is not True:
        raise ValueError('observation did not pass strict equality')


def prepare(report_bytes):
    report = json.loads(report_bytes)
    if not report['executed'] or not report['online_passed']:
        raise ValueError('requires actual passing original scratch report')
    if report['scratch_root'] != SCRATCH_ROOT:
        raise ValueError('unexpected frozen scratch root')
    rows = []
    for case in report['results']:
        if not case['passed']:
            raise ValueError('scratch case failed')
        for original in case['observations_get_only']:
            for key, size in [('object_id_full64', 64), ('file_id_info24', 24)]:
                if len(bytes.fromhex(original[key])) != size:
                    raise ValueError('incomplete original scratch observation')
            path = validate_selector(original['path_utf16_le'])
            if path != original['path']:
                raise ValueError('original path disagrees with raw selector')
            rows.append({key: original[key] for key in ('path_utf16_le', 'object_id_full64', 'file_id_info24')})
    if not rows:
        raise ValueError('no existing observations')
    return {'schema': 1, 'mode': 'GET-only-existing-scratch', 'original_report_sha256': digest(report_bytes),
            'frozen_scratch_backing_sha256': 'e18af50347659268f1edc0a037b6c9b8b0614e9fd778bd0cdc6c8f9e7e27940b',
            'scratch_root': report['scratch_root'], 'distinct_dos_alias': any(c['distinct_dos_alias'] for c in report['results']),
            'rows': rows}


def validate_report(expected, actual, input_hash):
    if actual['input_sha256'].lower() != input_hash:
        raise ValueError('input hash changed')
    if actual['mode'] != 'GET-only-existing-scratch' or len(actual['rows']) != len(expected['rows']):
        raise ValueError('observer scope/count changed')
    for left, right in zip(expected['rows'], actual['rows'], strict=True):
        validate_row(left, right)
    if actual['all_passed'] is not True:
        raise ValueError('observer failed')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='mode', required=True)
    make = commands.add_parser('prepare'); make.add_argument('original_report', type=Path); make.add_argument('output', type=Path)
    check = commands.add_parser('validate'); check.add_argument('input', type=Path); check.add_argument('report', type=Path)
    args = parser.parse_args()
    if args.mode == 'prepare':
        data = prepare(args.original_report.read_bytes())
        with args.output.open('x') as output:
            json.dump(data, output, indent=2); output.write('\n')
    else:
        raw = args.input.read_bytes()
        validate_report(json.loads(raw), json.loads(args.report.read_bytes()), digest(raw))
        print('GET-only observation exact')
