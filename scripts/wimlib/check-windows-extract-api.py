#!/usr/bin/env python3
"""Measure original Windows extraction on preserved native-written WIMs."""
import argparse
import hashlib
import json
import pathlib
import time
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--case-index', type=int, action='append', help='Run selected indices from the unchanged 48-case matrix')
    parser.add_argument('--fixture-path', help='Use one additional preserved guest WIM for focused extraction cases')
    parser.add_argument('--target-cases', action='store_true', help='Add relative, extended, long and existing read-only target cases')
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-extract/original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-extract-20261003'
    assert guest.powershell("New-Item -ItemType Directory -Force '" + root + "' | Out-Null")['exit'] == 0
    sources = {'default-acl': r'C:\wim-capture-20261003\native-default-write\capture-é漢.wim',
               'no-acl': r'C:\wim-capture-20261003\native-raw-root\capture-é漢.wim',
               'protected-acl': r'C:\wim-capture-20261003\native-protected\capture-é漢.wim'}
    if args.fixture_path:
        sources = {'default-acl': args.fixture_path}
    fixtures = {}
    for label, source in sources.items():
        data = guest.get(source)
        target = root + '\\' + (hashlib.sha256(data).hexdigest() if args.fixture_path else label) + '.wim'
        # Snapshot inputs once; subsequent runs must use exactly those bytes.
        exists = guest.powershell("if(Test-Path -LiteralPath '" + target + "'){Write-Output 'exists'}")
        if 'exists' in exists['stdout']:
            assert guest.get(target) == data, 'preserved extraction snapshot differs'
        else:
            guest.put(target, data)
        fixtures[label] = {'path': target, 'source': source, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}
    run = root + '\\' + args.implementation + '-' + str(time.time_ns())
    assert guest.powershell("New-Item -ItemType Directory -Force '" + run + "' | Out-Null")['exit'] == 0
    dll = args.dll.read_bytes()
    caller = pathlib.Path('target/windows-abi-probe/probe-windows-extract.exe').read_bytes()
    guest.put(run + r'\wim.dll', dll)
    guest.put(run + r'\probe.exe', caller)
    cases = [(label, flags, image, -1, 0, 'directory') for label in sources
             for flags in [0, 0x40, 0x80, 0x100000, 0x2000, 0x4000, 0x200, 0x100]
             for image in [1]]
    cases += [('default-acl', flags, image, -1, 0, 'directory')
              for flags, image in [(1, 1), (0x20, 1), (0xc0, 1), (0x300, 1),
                                   (0x400, 1), (0x40000000, 1), (0, -2), (0, -1), (0, 0), (0, 2)]]
    cases += [('default-acl', 0, 1, msg, value, 'directory') for msg in [0, 3, 4, 6, 7] for value in [1, 2]]
    cases += [('default-acl', 0, 1, -1, 0, kind) for kind in ['null', 'empty', 'file', 'parent-file']]
    if args.fixture_path:
        cases += [('default-acl', 0, 1, msg, value, 'directory') for msg in [303, 306] for value in [1, 2]]
    if args.target_cases:
        cases += [('default-acl', 0, 1, stop, status, kind) for kind in ['relative', 'extended', 'long', 'readonly', 'long-extended']
                  for stop, status in [(-1, 0), (0, 1), (3, 1)]]
    observations = []
    for index, (label, flags, image, stop, status, kind) in enumerate(cases):
        if args.case_index is not None and index not in args.case_index:
            continue
        target = run + '\\case-' + str(index) + '-é漢'
        if kind == 'null':
            target = '-null'
        elif kind == 'empty':
            target = '-empty'
        elif kind == 'file':
            assert guest.powershell("[IO.File]::WriteAllBytes('" + target + "',[byte[]](7,8,9))")['exit'] == 0
        elif kind == 'parent-file':
            assert guest.powershell("[IO.File]::WriteAllBytes('" + target + "',[byte[]](7,8,9))")['exit'] == 0
            target += r'\child'
        elif kind == 'readonly':
            assert guest.powershell("New-Item -ItemType Directory '" + target + "'|Out-Null;$d=[DateTime]::SpecifyKind([DateTime]::Parse('2010-01-02T03:04:05'),[DateTimeKind]::Utc);$f=Get-Item '" + target + "';$f.CreationTimeUtc=$d;$f.LastWriteTimeUtc=$d;$f.Attributes=[IO.FileAttributes]17")['exit'] == 0
        caller_target = target
        if kind == 'relative':
            caller_target = '-relative:' + target
        elif kind == 'extended':
            target = '\\\\?\\' + target
            caller_target = target
        elif kind in ['long', 'long-extended']:
            # Each target case owns a different parent chain.
            parent = run + '-long-' + str(index)
            caller_target = ('-long-extended:' if kind == 'long-extended' else '-long:') + parent
            assert guest.powershell("New-Item -ItemType Directory '" + parent + "'|Out-Null")['exit'] == 0
            target = '\\\\?\\' + parent + ''.join('\\' + letter * 100 for letter in 'abc') + r'\target'
        result = guest.execute(run + r'\probe.exe', [run + r'\wim.dll', fixtures[label]['path'], caller_target,
                               str(flags), str(image), str(stop), str(status)])
        assert not result['stdout_truncated']
        inventory = None
        if kind not in ['null', 'empty']:
            command = r'''
$r='TARGET';
if(Test-Path -LiteralPath $r) {
 Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles; public class WimExtractInfo { [StructLayout(LayoutKind.Sequential)] public struct Info {public uint Attr,Clo,Chi,Alo,Ahi,Wlo,Whi,Volume,SizeHi,SizeLo,Links,IndexHi,IndexLo;} [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info i); }';
 @(Get-Item -LiteralPath $r -Force)+@(if((Get-Item -LiteralPath $r -Force).PSIsContainer){Get-ChildItem -LiteralPath $r -Recurse -Force}) | ForEach-Object {
  $links=$null;if(-not $_.PSIsContainer){$f=[IO.File]::Open($_.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]7);$i=New-Object WimExtractInfo+Info;if(-not[WimExtractInfo]::GetFileInformationByHandle($f.SafeFileHandle,[ref]$i)){throw 'file information failed'};$links=$i.Links;$f.Dispose()};
  [PSCustomObject]@{Path=$_.FullName.Substring($r.Length);Directory=$_.PSIsContainer;Attrs=[int]$_.Attributes;Creation=$_.CreationTimeUtc.Ticks;Write=$_.LastWriteTimeUtc.Ticks;SDDL=(Get-Acl -LiteralPath $_.FullName).Sddl;Links=$links;Hash=$(if(-not $_.PSIsContainer){(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash})}
 } | ConvertTo-Json -Compress
} else {Write-Output 'null'}
'''.replace('TARGET', target)
            manifest = guest.powershell(command)
            assert manifest['exit'] == 0, manifest
            inventory = json.loads(manifest['stdout'])
            if isinstance(inventory, dict):
                inventory = [inventory]
            if args.fixture_path and inventory:
                shortnames = guest.powershell("Add-Type -TypeDefinition 'using System;using System.Text;using System.Runtime.InteropServices;public class WimShort { [DllImport(\"kernel32.dll\",CharSet=CharSet.Unicode,SetLastError=true)]public static extern uint GetShortPathName(string p,StringBuilder b,uint n); }';$r='" + target + "';@(Get-Item -LiteralPath $r -Force)+@(Get-ChildItem -LiteralPath $r -Recurse -Force)|ForEach-Object{$b=New-Object Text.StringBuilder 32768;$n=[WimShort]::GetShortPathName($_.FullName,$b,32768);if($n -eq 0){throw 'GetShortPathName failed'};[PSCustomObject]@{Path=$_.FullName.Substring($r.Length);ShortFilename=[IO.Path]::GetFileName($b.ToString())}}|ConvertTo-Json -Compress")
                assert shortnames['exit'] == 0, shortnames
                names = json.loads(shortnames['stdout'])
                if isinstance(names, dict):
                    names = [names]
                by_path = {row['Path']: row['ShortFilename'] for row in names}
                inventory = [dict(node, ShortFilename=by_path[node['Path']]) for node in inventory]
        observations.append({'fixture': label, 'flags': flags, 'image': image, 'stop': stop, 'status': status,
                             'target_kind': kind, 'result': result, 'inventory': inventory})
        # A live checkpoint makes early actual failures reviewable while the
        # remaining guest processes run. This is never the final evidence file.
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.with_suffix('.partial.json').write_text(json.dumps({'implementation': args.implementation,
            'run_folder': run, 'fixtures': fixtures, 'dll_sha256': hashlib.sha256(dll).hexdigest(),
            'caller_sha256': hashlib.sha256(caller).hexdigest(), 'incomplete': True, 'cases': observations}, indent=2) + '\n')
    for fixture in fixtures.values():
        assert hashlib.sha256(guest.get(fixture['source'])).hexdigest() == fixture['sha256']
        assert hashlib.sha256(guest.get(fixture['path'])).hexdigest() == fixture['sha256']
    result = {'scope': 'Actual Windows extraction callbacks/errors and NTFS attributes/times/hardlinks/SDDL; source WIMs preserved',
              'implementation': args.implementation, 'fixtures': fixtures, 'dll_sha256': hashlib.sha256(dll).hexdigest(),
              'caller_sha256': hashlib.sha256(caller).hexdigest(), 'source_preserved': True, 'cases': observations}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        result['differences'] = []
        key_fields = ['fixture', 'flags', 'image', 'stop', 'status', 'target_kind']
        by_key = {tuple(case[field] for field in key_fields): case for case in baseline['cases']}
        assert len(by_key) == len(baseline['cases']), 'ambiguous baseline cases'
        for new in observations:
            old = by_key[tuple(new[field] for field in key_fields)]
            left, right = dict(old), dict(new)
            # Partial-operation filesystem timestamps are the actual wallclock;
            # final restored metadata timestamps stay exact.
            if (old['stop'] not in [-1, 7] or old['target_kind'] == 'file') and left['inventory'] and right['inventory']:
                left['inventory'] = [{k: v for k, v in node.items() if k not in ['Creation', 'Write']} for node in left['inventory']]
                right['inventory'] = [{k: v for k, v in node.items() if k not in ['Creation', 'Write']} for node in right['inventory']]
            if old['image'] == -1 and left['inventory'] and right['inventory']:
                left['inventory'] = [{k: v for k, v in node.items() if node['Path'] != '' or k not in ['Creation', 'Write']} for node in left['inventory']]
                right['inventory'] = [{k: v for k, v in node.items() if node['Path'] != '' or k not in ['Creation', 'Write']} for node in right['inventory']]
            if left != right:
                result['differences'].append({'case': len(result['differences']), 'original': old, 'native': new})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'source_preserved': True,
                      'differences': len(result.get('differences', []))}, indent=2))


if __name__ == '__main__':
    main()
