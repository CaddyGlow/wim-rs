#!/usr/bin/env python3
"""Inventory every source WIM process-trust ACE and compare an offline target.

Decode the verified source with independent libwim; target descriptors come from
disk-capture's read-only trust_inventory example. No security transforms are hidden.
First run without --target to export --paths as JSON arrays of original UTF-16
code units for selected offline traversal. Target identity uses canonical
little-endian UTF-16 hex, preserving unpaired surrogates; display is advisory.
"""
import argparse
import collections
import importlib.util
import json
from pathlib import Path
import struct

spec = importlib.util.spec_from_file_location('whole', Path(__file__).with_name('compare-whole-wim-metadata.py'))
whole = importlib.util.module_from_spec(spec)
spec.loader.exec_module(whole)


def trust_aces(acl_hex, ace_type=20):
    data = bytes.fromhex(acl_hex or '')
    if not data:
        return []
    if len(data) < 8 or struct.unpack_from('<H', data, 2)[0] != len(data):
        raise ValueError('invalid ACL bounds')
    count = struct.unpack_from('<H', data, 4)[0]
    cursor = 8
    result = []
    for _ in range(count):
        if cursor + 4 > len(data):
            raise ValueError('truncated ACE')
        size = struct.unpack_from('<H', data, cursor + 2)[0]
        if size < 4 or cursor + size > len(data):
            raise ValueError('invalid ACE bounds')
        if data[cursor] == ace_type:
            result.append(data[cursor:cursor + size].hex())
        cursor += size
    return result


def without_inherited(aces):
    result = []
    for ace in aces:
        raw = bytearray.fromhex(ace)
        raw[1] &= ~0x10
        result.append(raw.hex())
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source_wim', type=Path)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--paths', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--target', type=Path)
    parser.add_argument('--ace-type', type=int, choices=[18, 20], default=20,
                        help='Reserved SACL ACE type: 20 process trust, 18 resource attribute.')
    args = parser.parse_args()
    nodes, _, _ = whole.inventory(args.source_wim, args.library)
    source = {}
    for key, node in nodes.items():
        security = node['security']
        aces = trust_aces(security['sacl'], args.ace_type) if security else []
        if aces:
            source[key] = {'display_path': node['path'], 'attributes': node['attributes'], 'trust_aces': aces, 'security': security}
    archive = whole.layout.Archive(args.source_wim, args.library)
    try:
        data = archive.metadata
        count = whole.layout.u32(data, 4)
        cursor = 8 + count * 8
        raw_descriptors = []
        for index in range(count):
            size = whole.layout.u64(data, 8 + index * 8)
            raw_descriptors.append(data[cursor:cursor + size].hex())
            cursor += size
        for path, _, _, _, _, record in archive.nodes_with_raw_records():
            key = path.encode('utf-16-le', 'surrogatepass').hex()
            if key in source:
                security_id = whole.layout.u32(record, 12)
                source[key]['raw_security_descriptor'] = raw_descriptors[security_id]
    finally:
        archive.close()
    args.paths.write_text(json.dumps([list(struct.unpack('<' + 'H' * (len(key) // 4), bytes.fromhex(key))) for key in sorted(source)], indent=2) + '\n')
    report = {'source_acl_ace_type': args.ace_type, 'source_nodes': len(nodes), 'source_trust_paths': len(source), 'source': source}
    if args.target:
        if args.target.stat().st_size > 64 * 1024 * 1024:
            raise ValueError('target descriptor report exceeds budget')
        target = {}
        for row in json.loads(args.target.read_text()):
            key = row['path_utf16_le']
            if bytes.fromhex(key).hex() != key or len(key) % 4:
                raise ValueError('noncanonical UTF16 path key')
            if key in target:
                raise ValueError('duplicate target path')
            security = whole.descriptor_components(bytes.fromhex(row['security']))
            target[key] = {'trust_aces': trust_aces(security['sacl'], args.ace_type), 'security': security}
        counts = collections.Counter()
        differences = []
        for path, node in sorted(source.items()):
            applied = target.get(path)
            if applied is None:
                kind = 'missing_path'
            elif node['trust_aces'] == applied['trust_aces']:
                kind = 'exact'
            elif without_inherited(node['trust_aces']) == without_inherited(applied['trust_aces']):
                kind = 'inherited_flag_only'
            elif not applied['trust_aces']:
                kind = 'missing_ace'
            else:
                kind = 'changed_ace'
            counts[kind] += 1
            if kind != 'exact':
                parent_display = node['display_path'].rsplit('/', 1)[0]
                parent = parent_display.encode('utf-16-le', 'surrogatepass').hex()
                differences.append({'path_utf16_le': path, 'display_path': node['display_path'], 'difference': kind, 'source': node,
                                    'target': applied, 'parent': parent,
                                    'source_parent': nodes.get(parent),
                                    'target_parent': target.get(parent)})
        report.update({'counts': dict(counts), 'differences': differences})
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: value for key, value in report.items() if key not in ('source', 'differences')}))


if __name__ == '__main__':
    main()
