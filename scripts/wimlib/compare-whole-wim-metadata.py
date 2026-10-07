#!/usr/bin/env python3
"""Compare complete raw WIM trees; report differences without relaxing policy.

Upstream libwim decodes resources. Logical stream SHA-1 IDs are compared after
independent archive verification, never treated as proof of verification here.
"""
import argparse
import collections
import importlib.util
import json
from pathlib import Path
import struct
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('layout', Path(__file__).with_name('check-qcow2-reparse-layout.py'))
layout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout)


def descriptor_components(raw):
    if len(raw) < 20:
        raise ValueError('short security descriptor')
    parts = {'control': struct.unpack_from('<H', raw, 2)[0]}
    for name, at in [('owner', 4), ('group', 8), ('sacl', 12), ('dacl', 16)]:
        offset = layout.u32(raw, at)
        if not offset:
            parts[name] = None
            continue
        if offset + 8 > len(raw):
            raise ValueError('security component outside descriptor')
        size = 8 + 4 * raw[offset + 1] if name in ('owner', 'group') else layout.u16(raw, offset + 2)
        if offset + size > len(raw):
            raise ValueError('truncated security component')
        parts[name] = raw[offset:offset + size].hex()
    return parts


def inventory(path, library):
    archive = layout.Archive(path, library)
    try:
        data = archive.metadata
        count = layout.u32(data, 4)
        cursor = 8 + count * 8
        securities = []
        for i in range(count):
            size = layout.u64(data, 8 + i * 8)
            securities.append(descriptor_components(data[cursor:cursor + size]))
            cursor += size
        nodes = {}
        groups = collections.defaultdict(list)
        for path, attrs, main, extras, inode, raw in archive.nodes_with_raw_records():
            security_id = layout.u32(raw, 12)
            security = None if security_id == 0xffffffff else securities[security_id]
            name_len, short_len = layout.u16(raw, 100), layout.u16(raw, 98)
            short_start = 102 + name_len + (2 if name_len else 0)
            short = raw[short_start:short_start + short_len].hex()
            tag_start = (short_start + short_len + (2 if short_len else 0) + 7) & ~7
            tags = []
            while tag_start + 8 <= len(raw):
                tag, size = layout.u32(raw, tag_start), layout.u32(raw, tag_start + 4)
                if not tag and not size:
                    break
                if tag_start + 8 + size > len(raw):
                    raise ValueError('invalid tagged metadata length')
                tags.append((tag, raw[tag_start + 8:tag_start + 8 + size].hex()))
                tag_start = (tag_start + 8 + size + 7) & ~7
            streams = []
            all_streams = ([(main, '')] if main != layout.ZERO else []) + extras
            unnamed = 0
            for digest, name in all_streams:
                kind = 'ADS' if name else 'REPARSE' if attrs & 1024 and unnamed == 0 else 'DATA'
                if not name:
                    unnamed += 1
                size = archive.blobs[digest][3] if digest != layout.ZERO else 0
                streams.append((kind, name.encode('utf-16-le', 'surrogatepass').hex(), size, digest.hex()))
            if not attrs & 0x410 and not any(item[0] == 'DATA' for item in streams):
                streams.append(('DATA', '', 0, layout.ZERO.hex()))
            key = path.encode('utf-16-le', 'surrogatepass').hex()
            nodes[key] = {'path': path, 'attributes': attrs, 'times': [layout.u64(raw, n) for n in (40, 48, 56)], 'short_name_utf16': short, 'security': security, 'tags': sorted(tags), 'streams': sorted(streams)}
            if inode and not attrs & 0x410:
                groups[inode].append(key)
        links = sorted(sorted(paths) for paths in groups.values() if len(paths) > 1)
        xml = ET.fromstring(archive.resource(layout.descriptor(archive.read(72, 24))))
        windows = xml.find("IMAGE/WINDOWS")
        properties = None if windows is None else [(element.tag, dict(element.attrib), (element.text or "").strip()) for element in windows.iter()]
        return nodes, links, properties
    finally:
        archive.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('native', type=Path)
    parser.add_argument('independent', type=Path)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    a, ah, ax = inventory(args.native, args.library)
    b, bh, bx = inventory(args.independent, args.library)
    differences = []
    counts = collections.Counter()
    for key in sorted(a.keys() & b.keys()):
        changed = {field: {'native': a[key][field], 'independent': b[key][field]} for field in a[key] if a[key][field] != b[key][field]}
        if changed:
            counts.update(changed.keys())
            differences.append({'path': a[key]['path'], 'changes': changed})
    result = {'native_nodes': len(a), 'independent_nodes': len(b), 'native_only': [a[k]['path'] for k in sorted(a.keys() - b.keys())], 'independent_only': [b[k]['path'] for k in sorted(b.keys() - a.keys())], 'difference_counts': dict(counts), 'differences': differences, 'native_hardlink_groups': ah, 'independent_hardlink_groups': bh, 'hardlink_groups_equal': ah == bh, 'native_windows_properties': ax, 'independent_windows_properties': bx, 'windows_properties_equal': ax == bx, 'policy': 'No transformations silently ignored; stream hashes require separate whole-archive verification.'}
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=True) + '\n')
    print(json.dumps({k: result[k] for k in ('native_nodes', 'independent_nodes', 'difference_counts', 'hardlink_groups_equal')}))


if __name__ == '__main__':
    main()
