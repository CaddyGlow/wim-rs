"""Read-only correlation of scratch security observations and frozen native bytes."""
import argparse
import collections
import hashlib
import json
from pathlib import Path


def logical(text):
    raw = bytes.fromhex(text)
    if len(raw) < 20 or raw[0] != 1 or not int.from_bytes(raw[2:4], 'little') & 0x8000:
        raise ValueError('Self-relative SD header')
    control = int.from_bytes(raw[2:4], 'little')
    result = {'control': control}
    for name, field in [('owner', 4), ('group', 8), ('sacl', 12), ('dacl', 16)]:
        offset = int.from_bytes(raw[field:field+4], 'little')
        if offset and (offset < 20 or offset + 8 > len(raw)):
            raise ValueError('SD component offset')
        if name in ('owner', 'group'):
            size = 8 + 4 * raw[offset+1] if offset else 0
            if offset + size > len(raw):
                raise ValueError('SID bounds')
            result[name] = raw[offset:offset+size].hex() if offset else ''
            continue
        aces = []
        revision = None
        if offset:
            size = int.from_bytes(raw[offset+2:offset+4], 'little')
            if size < 8 or offset + size > len(raw):
                raise ValueError('ACL bounds')
            revision = raw[offset]
            cursor = offset + 8
            for _ in range(int.from_bytes(raw[offset+4:offset+6], 'little')):
                if cursor + 4 > offset + size:
                    raise ValueError('ACE header bounds')
                length = int.from_bytes(raw[cursor+2:cursor+4], 'little')
                if length < 4 or length % 4 or cursor + length > offset + size:
                    raise ValueError('ACE bounds')
                aces.append(dict(type=raw[cursor], flags=raw[cursor+1], raw=raw[cursor:cursor+length].hex()))
                cursor += length
        result[name] = dict(present=bool(control & (4 if name == 'dacl' else 16)),
                            null=not bool(offset), revision=revision, aces=aces)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    args = parser.parse_args(); base = args.directory
    report = json.loads((base/'actual-controls-report.json').read_bytes())
    native = json.loads((base/'native-all180-metadata.json').read_bytes())
    by = {r['path_utf16_le']: r for r in native['rows']}
    basic = {r['file_reference']: r for r in json.loads((base/'native-all150-basic4.json').read_bytes())['rows']}
    if len(by) != 180 or len(report['results']) != 60 or not native['strict_clean_ntfs_open']:
        raise ValueError('Unexpected exact scratch scope or unclean volume')
    rows = []; counts = collections.defaultdict(collections.Counter)
    for index, case in enumerate(report['results']):
        observations = []
        for field in ('after_reopen', 'related_after', 'sibling_after'):
            runtime = case[field]
            selector = runtime['path'][2:].replace('\\', '/').encode('utf-16le').hex()
            actual = by[selector]; raw_basic = basic[actual['file_reference']]
            checks = {k: runtime['basic4'][k] == raw_basic[k] for k in ('creation_time', 'access_time', 'write_time', 'change_time')}
            identity = bytes.fromhex(runtime['file_id_info24'])
            if len(identity) != 24:
                raise ValueError('FILE_ID_INFO bounds')
            checks.update(attributes=runtime['basic4']['attributes'] == actual['attributes'],
                          own_target_reference=int.from_bytes(identity[8:], 'little') == actual['file_reference'],
                          own_target_volume=int.from_bytes(identity[:8], 'little') == native['volume_serial'],
                          owner_group_dacl_sacl_control=runtime['logical_sd'] == logical(actual['security_raw']))
            observations.append(dict(role=field, path_utf16_le=selector, checks=checks,
                                     runtime_raw_sd=runtime['raw_sd'], native_raw_sd=actual['security_raw'],
                                     raw_packing_exact=runtime['raw_sd'] == actual['security_raw'], native_standard_info=raw_basic))
        main_sd = logical(by[observations[0]['path_utf16_le']]['security_raw'])
        challenge = case['challenge']['ace_hex']
        ace_exact = bool(challenge and any(a['raw'] == challenge for a in main_sd['sacl']['aces']))
        counts[case['api']]['cases'] += 1
        counts[case['api']]['runtime_frozen_all3_logical_exact'] += all(all(o['checks'].values()) for o in observations)
        counts[case['api']]['source_ACE_exact'] += ace_exact
        counts[case['api']]['native_desired_full_logical_security_exact'] += main_sd == case['desired_logical_sd']
        counts[case['api']]['strict_online_pass'] += case['passed']
        rows.append(dict(case_index=index, api=case['api'], kind=case['kind'], challenge=case['challenge'],
                         native_source_ACE_exact=ace_exact, native_desired_logical_security_exact=main_sd == case['desired_logical_sd'],
                         original_strict_passed=case['passed'], observations=observations))
    result = dict(scope='Strict-clean frozen scratch rawNTFS180paths/150refs compared within SAME target volume to actual Windows observations. No projection inference.',
                  all180_runtime_frozen_logical_exact=all(all(o['checks'].values()) for r in rows for o in r['observations']),
                  counts=counts, rows=rows, production_executable=False,
                  remaining_reserved_mask_scope='0x1008f/0x101ff plus protection not tested in these60cases; no all-mask unsupported claim',
                  input_sha256={f: hashlib.sha256((base/f).read_bytes()).hexdigest() for f in
                                ['actual-controls-report.json', 'native-all180-metadata.json', 'native-all150-basic4.json', 'immutable-security-child-handoff.json']})
    output = base/'complete-native-online-correlation.json'
    with output.open('x') as stream:
        json.dump(result, stream, indent=2); stream.write('\n')
    print(json.dumps(dict(all180_runtime_frozen_logical_exact=result['all180_runtime_frozen_logical_exact'], counts=counts,
                         sha256=hashlib.sha256(output.read_bytes()).hexdigest()), indent=2))


if __name__ == '__main__':
    main()
