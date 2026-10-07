#!/usr/bin/env python3
"""Compare representative payloads and whole-tree metadata after independent apply."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import runpy
import zipfile

from windows_guest import WindowsGuest

SAMPLES = r"""
$ErrorActionPreference='Stop';$root='ROOT';
@('System32\ntdll.dll','System32\kernel32.dll','System32\drivers\ACPI.sys','System32\config\SYSTEM','System32\config\SOFTWARE')|ForEach-Object{
$p=$root+'\'+$_;$i=[MetadataProbe]::Information($p);
[PSCustomObject]@{Path=$_;Info=[PSCustomObject]@{Attributes=$i.Attributes;Creation=$i.Creation;Access=$i.Access;Write=$i.Write};Security=[MetadataProbe]::Security($p);Short=[MetadataProbe]::ShortName($p);Streams=@([MetadataProbe]::Streams($p)|Sort-Object Name)}
}|ConvertTo-Json -Depth 8 -Compress
"""


def security_parts(line):
    raw = bytes.fromhex(line.split()[2])
    parts = {}
    for name, offset in [('owner', 4), ('group', 8), ('sacl', 12), ('dacl', 16)]:
        start = int.from_bytes(raw[offset:offset + 4], 'little')
        size = (8 + raw[start + 1] * 4 if name in ['owner', 'group'] else
                int.from_bytes(raw[start + 2:start + 4], 'little')) if start else 0
        parts[name] = raw[start:start + size] if start else b''
    return int.from_bytes(raw[2:4], 'little'), parts


def known_dism_difference(before, after):
    if len(before) != len(after):
        return False
    for original, applied in zip(before, after):
        if original == applied:
            continue
        left, right = original.split(), applied.split()
        if left[0] == right[0] == 'node':
            # DISM marks restored files archived and replaces the NORMAL sentinel.
            if left[:2] != right[:2] or left[3:] != right[3:]:
                return False
            if int(right[2]) != (int(left[2]) & ~128) | 32:
                return False
        elif left[0] == right[0] == 'dos':
            filename = bytes.fromhex(before[0].split()[1]).decode('utf-16-be', 'surrogatepass').rsplit('\\', 1)[-1]
            if right[1] != 'null' or bytes.fromhex(left[1]).decode('utf-16-be', 'surrogatepass') != filename:
                return False
        elif left[0] == right[0] == 'sd':
            control_before, parts_before = security_parts(original)
            control_after, parts_after = security_parts(applied)
            if (control_before ^ control_after) & ~(0x10 | 0x800):
                return False
            for name in ['owner', 'group', 'dacl']:
                if parts_before[name] != parts_after[name]:
                    return False
            if parts_before['sacl'] != parts_after['sacl']:
                empty_acl = parts_before['sacl']
                if not (len(empty_acl) == 8 and empty_acl[4:6] == b'\0\0' and not parts_after['sacl']):
                    return False
        else:
            return False
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--volume-evidence', required=True, type=Path)
    parser.add_argument('--probe', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    helper = runpy.run_path(str(Path(__file__).with_name('check-windows-metadata-reliability.py')))['HELPER']
    guest = WindowsGuest(args.qga_socket)
    evidence = json.loads(args.volume_evidence.read_text())
    root = evidence['fixture']
    target = root + r'\windows-dism'

    def inventory(directory):
        result = guest.powershell(helper + SAMPLES.replace('ROOT', directory))
        if result['exit'] != 0 or result.get('stdout_truncated'):
            raise RuntimeError(result)
        return json.loads(result['stdout'])

    evidence['sample_source'] = inventory(evidence['source'])
    evidence['sample_applied'] = inventory(target)
    evidence['samples_equal'] = evidence['sample_source'] == evidence['sample_applied']
    args.volume_evidence.write_text(json.dumps(evidence, indent=2) + '\n')
    data = args.probe.read_bytes()
    guest.put(root + r'\post-tree.exe', data)
    post = {'source': evidence['source'], 'target': target, 'cases': [], 'artifacts': {
        'probe': hashlib.sha256(data).hexdigest(), 'original.dll': evidence['artifacts']['original.dll']}}
    inventories = []
    for label, directory in [('source', post['source']), ('applied', target)]:
        log = root + '\\post-' + label + '.txt'
        archive = log + '.zip'
        command = ("$ErrorActionPreference='Stop';& '" + root + "\\post-tree.exe' '" + root +
                   "\\original.dll' '" + directory + "' '-' '64' '-' '-1' '0' '12' > '" + log +
                   "';$rc=$LASTEXITCODE;Compress-Archive -Force -LiteralPath '" + log +
                   "' -DestinationPath '" + archive + "';Get-Content -LiteralPath '" + log + "' -Tail 1;exit $rc")
        result = guest.execute(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
                               ['-NoProfile', '-NonInteractive', '-Command', command], timeout=1800)
        result.pop('stdout_base64', None)
        with zipfile.ZipFile(io.BytesIO(guest.get(archive))) as zipped:
            lines = zipped.read(zipped.namelist()[0]).decode('utf-16').splitlines()
        nodes = {}
        current = None
        for line in lines:
            if line.startswith('node '):
                current = line.split()[1]
                if current in nodes:
                    raise RuntimeError('duplicate manifest path')
                nodes[current] = [line]
            elif current and line.startswith(('dos ', 'sd ', 'stream ')):
                nodes[current].append(line)
        success = (result['exit'] == 0 and not result.get('stdout_truncated') and
                   'init 0' in lines and 'add 0' in lines and 'tree 0' in lines and bool(nodes))
        post['cases'].append({'label': label, 'result': result, 'nodes': len(nodes), 'success': success})
        inventories.append(nodes)
    original, applied = inventories
    differences = [{'path': path, 'source': original[path], 'applied': applied[path]}
                   for path in sorted(original.keys() & applied.keys()) if original[path] != applied[path]]
    post.update({'missing_count': len(original.keys() - applied.keys()),
                 'extra_count': len(applied.keys() - original.keys()), 'difference_count': len(differences),
                 'differences': differences, 'exact': original == applied})
    post['known_transformations_only'] = all(known_dism_difference(d['source'], d['applied']) for d in differences)
    post['passes'] = (evidence['samples_equal'] and all(c['success'] for c in post['cases']) and
                      not post['missing_count'] and not post['extra_count'] and post['known_transformations_only'])
    args.output.write_text(json.dumps(post, indent=2) + '\n')
    print(json.dumps({'passes': post['passes'], 'exact': post['exact'], 'differences': len(differences)}))
    return 0 if post['passes'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
