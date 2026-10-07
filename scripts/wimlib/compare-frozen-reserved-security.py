#!/usr/bin/env python3
"""Compare all frozen source reserved challenges with selected offline descriptors."""
import argparse
import collections
import hashlib
import importlib.util
import json
from pathlib import Path
import struct

spec = importlib.util.spec_from_file_location('whole', Path(__file__).with_name('compare-whole-wim-metadata.py'))
whole = importlib.util.module_from_spec(spec)
spec.loader.exec_module(whole)


def aces(raw, kind):
    if raw is None:
        return []
    acl = bytes.fromhex(raw)
    if len(acl) < 8 or struct.unpack_from('<H', acl, 2)[0] != len(acl):
        raise ValueError('invalid ACL bounds')
    result = []
    cursor = 8
    for _ in range(struct.unpack_from('<H', acl, 4)[0]):
        if cursor + 4 > len(acl):
            raise ValueError('truncated ACE')
        size = struct.unpack_from('<H', acl, cursor + 2)[0]
        if size < 4 or cursor + size > len(acl):
            raise ValueError('invalid ACE bounds')
        if acl[cursor] == kind:
            result.append(acl[cursor:cursor + size].hex())
        cursor += size
    return result


def clear_inherited(items):
    result = []
    for item in items:
        raw = bytearray.fromhex(item)
        raw[1] &= ~16
        result.append(raw.hex())
    return result


def compare(source, target, original):
    target_by_path = {r['path_utf16_le']: r for r in target}
    if len(target_by_path) != len(target):
        raise ValueError('duplicate target descriptor paths')
    original_paths = {r['path_utf16_le'] for r in original['rows']}
    counts = collections.defaultdict(collections.Counter)
    rows = []
    for row in source['rows']:
        key = row['path_utf16_le']
        group = 'original5727' if key in original_paths else 'current_additions'
        actual = target_by_path.get(key)
        kinds = {bytes.fromhex(a)[0] for a in row['reserved_aces']}
        if not kinds <= {18, 20}:
            raise ValueError('unexpected source reserved type')
        result = {'path_utf16_le': key, 'display_path': row['display_path'], 'group': group,
                  'source_security_raw': row['security'], 'target_security_raw': actual['security'] if actual else None}
        counts[group]['selected'] += 1
        if actual is None:
            counts[group]['missing_path'] += 1
            result['classification'] = 'missing_path'
        else:
            left = whole.descriptor_components(bytes.fromhex(row['security']))
            right = whole.descriptor_components(bytes.fromhex(actual['security']))
            changed = [name for name in left if left[name] != right[name]]
            result['component_differences'] = changed
            result['descriptor_components_exact'] = not changed
            result['raw_descriptor_exact'] = row['security'] == actual['security']
            for name in changed:
                counts[group]['component_changed_' + name] += 1
            counts[group]['raw_descriptor_exact' if result['raw_descriptor_exact'] else
                          'packing_only_difference' if not changed else 'descriptor_component_difference'] += 1
            result['reserved'] = {}
            for kind in sorted(kinds):
                before = aces(left['sacl'], kind)
                after = aces(right['sacl'], kind)
                state = 'exact' if before == after else 'missing' if not after else 'inherited_bit_only' if clear_inherited(before) == clear_inherited(after) else 'other_difference'
                result['reserved'][str(kind)] = {'classification': state, 'source': before, 'target': after}
                counts[group]['ace' + str(kind) + '_' + state] += 1
        rows.append(result)
    return {'groups': {k: dict(v) for k, v in counts.items()}, 'rows': rows,
            'reserved_ace_gate_passed': all(r.get('reserved') and all(a['classification'] == 'exact' for a in r['reserved'].values()) for r in rows),
            'descriptor_component_gate_passed': all(r.get('descriptor_components_exact', False) for r in rows),
            'original_paths_not_in_current_inventory': sorted(original_paths - {r['path_utf16_le'] for r in source['rows']}),
            'policy': 'Offsets/packing distinguished from actual owner/group/DACL/SACL/control changes. Inherited-bit-only differences remain failures of exact reserved ACE fidelity. No missing-path waiver.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'target', 'original', 'output'):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    inputs = {}; parsed = {}
    for name in ('source', 'target', 'original'):
        path = getattr(args, name); data = path.read_bytes()
        parsed[name] = json.loads(data)
        inputs[name] = {'path': str(path), 'sha256': hashlib.sha256(data).hexdigest()}
    result = compare(**parsed)
    result.update(schema=1, inputs=inputs)
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
    print(json.dumps(result['groups']))
