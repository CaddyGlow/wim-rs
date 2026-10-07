#!/usr/bin/env python3
"""Read-only comparison of two strict native index inventories; no identity waiver."""
import argparse
import collections
import hashlib
import json
from pathlib import Path


def load(path):
    raw = path.read_bytes()
    data = json.loads(raw)
    if not data['all_native_index_references_exact'] or data['count'] != len(data['aliases']):
        raise ValueError('incomplete native alias proof')
    return data, hashlib.sha256(raw).hexdigest()


def compare(source, target):
    source_by_path = {r['path_utf16_le']: r for r in source['aliases']}
    target_by_path = {r['path_utf16_le']: r for r in target['aliases']}
    target_by_short = {(r['parent_path_utf16_le'], r['alias_utf16_le']): r for r in target['aliases']}
    grouped = collections.defaultdict(list)
    for row in target['aliases']:
        grouped[(row['parent_path_utf16_le'], row['alias_utf16_le'])].append(row)
    if any(len({r['file_id'] for r in rows}) != 1 for rows in grouped.values()) or len(source_by_path) != len(source['aliases']):
        raise ValueError('ambiguous duplicate mapping')
    counts = collections.Counter()
    differences = []
    source_links = collections.Counter(r['file_id'] for r in source['aliases'])
    for path, row in source_by_path.items():
        actual = target_by_path.get(path)
        binding = target_by_short.get((row['parent_path_utf16_le'], row['alias_utf16_le']))
        if actual is None:
            classification = 'no_target_distinct_alias_record_for_long_path'
        elif row['alias_utf16_le'] == actual['alias_utf16_le'] and binding and binding['file_id'] == actual['file_id']:
            classification = 'exact_alias_binding'
        elif binding and binding['file_id'] != actual['file_id']:
            classification = 'expected_alias_binds_other_target_file'
        elif binding:
            classification = 'expected_alias_same_target_file_but_short_record_differs'
        else:
            classification = 'expected_alias_absent_from_target_alias_index'
        counts[classification] += 1
        if classification != 'exact_alias_binding':
            differences.append({'classification': classification, 'source': row,
                                'source_distinct_alias_link_count': source_links[row['file_id']],
                                'target_long_record': actual, 'target_expected_alias_binding': binding,
                                'target_expected_alias_same_id_long_paths': grouped.get((row['parent_path_utf16_le'], row['alias_utf16_le']), [])})
    return {'counts': dict(counts), 'differences': differences,
            'target_additional_distinct_alias_paths': [r for p, r in target_by_path.items() if p not in source_by_path],
            'full_source_alias_gate_passed': not differences,
            'limitation': 'Inventory may select a shared short record for multiple same-parent hardlinks; these are retained as same-ID bindings, not treated as collisions. An absent selected alias does not prove absence from the complete raw INDEX. No target alias record may mean missing path or no distinct DOS name; requires full path inventory to distinguish. File IDs are compared within each target volume only.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('target', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    source, source_hash = load(args.source)
    target, target_hash = load(args.target)
    result = compare(source, target)
    result.update(schema=1, source_inventory=str(args.source), source_sha256=source_hash,
                  target_inventory=str(args.target), target_sha256=target_hash,
                  source_nodes=source['nodes'], target_nodes=target['nodes'])
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
    print(json.dumps(result['counts']))
