#!/usr/bin/env python3
"""Validate isolated automatic-alias observations without crediting restoration."""
import json
import re
import sys


def validate(d):
    if not re.fullmatch(r"[A-Za-z]:\\QCOW2-AliasCreation-[0-9a-f]{32}", d['Root']):
        raise ValueError('invalid new scratch root')
    if d['GlobalPolicy'] != 2 or any(d[k] for k in ('EnableStatus', 'QueryBeforeStatus', 'QueryAfterStatus')):
        raise ValueError('volume policy prerequisite failed')
    if len(d['Rounds']) != 3 or {r['FirstLink'] for r in d['Rounds']} != {0, 1, 2}:
        raise ValueError('missing creation orders')
    results = []
    for r in d['Rounds']:
        baseline = r['Initial']
        for observations in [*[s['Observations'] for s in r['CreationSteps']], r['BeforeRename'], r['AfterRename']]:
            for o in observations:
                if not o['Path'].startswith(d['Root'] + '\\'):
                    raise ValueError('observation outside scratch')
                if bytes.fromhex(o['PathUtf16']).decode('utf-16-le') != o['Path']:
                    raise ValueError('path encoding mismatch')
                if not re.fullmatch('[0-9A-Fa-f]{48}', o['Identity24']):
                    raise ValueError('truncated identity')
                for field in ('Identity24', 'PayloadSHA256', 'SecurityOwnerGroupDacl'):
                    if o[field] != baseline[field]:
                        raise ValueError('file invariant changed: ' + field)
        for key in ('BeforeRenameEntries', 'AfterRenameEntries'):
            if len(r[key]) != 3:
                raise ValueError('incomplete parent enumeration')
            for e in r[key]:
                for field in ('LongName', 'Alias'):
                    if bytes.fromhex(e[field+'Utf16']).decode('utf-16-le') != e[field]:
                        raise ValueError('entry encoding mismatch')
                if e['LongID'] != baseline['Identity24'] or (e['Alias'] and e['AliasID'] != e['LongID']):
                    raise ValueError('entry resolves to different file')
        results.append({'first_link': r['FirstLink'], 'rename_status': r['RenameStatus'],
                        'all_creation_links_have_aliases': all(e['Alias'] for e in r['BeforeRenameEntries']),
                        'all_postrename_links_have_aliases': all(e['Alias'] for e in r['AfterRenameEntries']),
                        'raw_basic_standard_changed': any(a['BasicAndStandardRaw'] != b['BasicAndStandardRaw'] for a,b in zip(r['BeforeRename'],r['AfterRename']))})
    return {'rounds': results, 'installed_restoration_proven': False, 'sacl_tested': False}


if __name__ == '__main__':
    print(json.dumps(validate(json.load(open(sys.argv[1]))), indent=2))
