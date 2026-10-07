#!/usr/bin/env python3
"""Validate actual scratch observations; never infer a pass from an API status alone."""
import argparse
import json
import re
from pathlib import Path


def validate(report):
    root = report['Root']
    if not re.fullmatch(r'[A-Za-z]:\\(?:[^\\]+\\)*QCOW2-AliasScratch-[0-9a-f]{32}', root):
        raise ValueError('scratch root identity missing')
    if any(part in ('.', '..') for part in root[3:].split('\\')):
        raise ValueError('scratch root traversal')
    if 'RootUtf16' in report and bytes.fromhex(report['RootUtf16']).decode('utf-16-le') != root:
        raise ValueError('scratch root UTF16 mismatch')
    actions = report['Actions']
    if not actions:
        raise ValueError('missing actions')
    for action in actions:
        if not action['Path'].casefold().startswith(root.casefold() + '\\'):
            raise ValueError('action outside scratch')
        if bytes.fromhex(action['AliasUtf16']).decode('utf-16-le') != action['Alias']:
            raise ValueError('alias UTF16 mismatch')
        for observation in (action['Before'], action['After']):
            if not re.fullmatch('[0-9A-Fa-f]{48}', observation['Identity24']):
                raise ValueError('full 24-byte identity required')
            if observation['Path'] != action['Path']:
                raise ValueError('observation path mismatch')
        for field in ('Identity24', 'PayloadSHA256', 'SecurityOwnerGroupDacl'):
            if action['Before'][field] != action['After'][field]:
                raise ValueError('scratch invariant changed: ' + field)
    def mappings(rows, count, distinct=False):
        if len(rows) != count:
            raise ValueError('missing reopen mappings')
        for row in rows:
            if not re.fullmatch('[0-9A-Fa-f]{48}', row['LongID']) or row['LongID'] != row['ShortID']:
                raise ValueError('short alias resolves to wrong object')
        if distinct and count == 2 and rows[0]['LongID'] == rows[1]['LongID']:
            raise ValueError('pair challenge files must differ')
    mappings(report['CollisionMapping'], 2, distinct=True)
    mappings(report['PermutationMapping'], 2, distinct=True)
    if report['CollisionStatus'] != 183 or report['LongNameCollisionStatus'] != 183:
        raise ValueError('collision rejection not demonstrated')
    if any(report['SetupStatuses']) or any(report['PermutationStatuses']):
        raise ValueError('permutation API failed')
    hard = report['HardlinkInitial'] + report['HardlinkFinal']
    if len(hard) != 6 or len({r['Identity24'] for r in hard}) != 1:
        raise ValueError('hardlink identity not preserved')
    successes = sum(status == 0 for status in report['HardlinkStatuses'][3:])
    mappings(report['HardlinkMappings'], successes)
    if report['DirectoryStatus'] == 0:
        mappings([report['DirectoryMapping']], 1)
    ordering = report.get('HardlinkOrderingRounds')
    ordering_results = None
    if ordering is not None:
        ordering_results = validate_ordering(ordering)
    return {'hardlink_ordering': ordering_results, 'pair_permutation_passed': True,
            'hardlink_all_alias_setters_succeeded': not any(report['HardlinkStatuses']),
            'directory_alias_passed': report['DirectoryStatus'] == 0,
            'security_scope': 'owner/group/DACL only; SACL untested',
            'installed_alias_restoration_proven': False}


def validate_ordering(rounds):
    if len(rounds) != 3 or {r['FirstLink'] for r in rounds} != {0, 1, 2}:
        raise ValueError('missing hardlink ordering cases')
    results = []
    for row in rounds:
        if len(row['ClearStatuses']) != 3 or any(row['ClearStatuses']):
            raise ValueError('hardlink clearing failed')
        if len(row['EmptyParentEnumeration']) != 3 or any(e['Alias'] for e in row['EmptyParentEnumeration']):
            raise ValueError('prior aliases remain before ordering case')
        sets = row['SetResults']
        if len(sets) != 3 or sets[0]['LinkIndex'] != row['FirstLink'] or {s['LinkIndex'] for s in sets} != {0, 1, 2}:
            raise ValueError('incomplete hardlink setter order')
        if len(row['Before']) != 3 or len(row['After']) != 3:
            raise ValueError('missing hardlink invariant observations')
        for before, after in zip(row['Before'], row['After']):
            for field in ('Path', 'Identity24', 'PayloadSHA256', 'SecurityOwnerGroupDacl'):
                if before[field] != after[field]:
                    raise ValueError('hardlink invariant changed: ' + field)
            if not re.fullmatch('[0-9A-Fa-f]{48}', before['Identity24']):
                raise ValueError('full hardlink identity required')
        entries = row['FinalParentEnumeration']
        if len(entries) != 3:
            raise ValueError('missing final parent entries')
        for entry in row['EmptyParentEnumeration'] + entries:
            if bytes.fromhex(entry['LongNameUtf16']).decode('utf-16-le') != entry['LongName']:
                raise ValueError('long name UTF16 mismatch')
            if bytes.fromhex(entry['AliasUtf16']).decode('utf-16-le') != entry['Alias']:
                raise ValueError('alias UTF16 mismatch')
            if entry['Alias'] and entry['LongID'] != entry['AliasID']:
                raise ValueError('hardlink alias resolves to wrong object')
        results.append({'first_link': row['FirstLink'], 'setter_statuses': [s['Status'] for s in sets],
                        'all_three_aliases_preserved': all(s['Status'] == 0 for s in sets)
                        and all(e['Alias'] for e in entries)})
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    print(json.dumps(validate(json.loads(args.report.read_text())), indent=2))
