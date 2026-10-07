#!/usr/bin/env python3
"""Run unchanged-header capture contracts on a preserved, owned Windows fixture."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import struct
import tempfile
import time
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--probe', type=pathlib.Path, default=pathlib.Path('target/windows-abi-probe/probe-windows-capture.exe'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--case', help='Run one named case, retaining explicit scope')
    parser.add_argument('--write-case', default='plain-no-acls')
    parser.add_argument('--windows-reader', action='store_true', help='Verify/apply using independent original Windows DLL too')
    parser.add_argument('--protected-acl', action='store_true', help='Use separate fixture with explicit owner/group and protected allow/deny/inheritance')
    parser.add_argument('--init-flags', type=int)
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-capture/original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-capture-20261003'
    setup_command = r'''
$root='C:\wim-capture-20261003'; New-Item -ItemType Directory -Force $root | Out-Null;
$source=$root+'\source-é漢';
if(-not(Test-Path -LiteralPath $source)) {
 New-Item -ItemType Directory $source | Out-Null;
 New-Item -ItemType Directory ($source+'\sub') | Out-Null;
 [IO.File]::WriteAllBytes($source+'\a.bin',[Text.Encoding]::ASCII.GetBytes("shared-data`n"));
 New-Item -ItemType HardLink -Path ($source+'\link.bin') -Target ($source+'\a.bin') | Out-Null;
 New-Item -ItemType HardLink -Path ($source+'\sub\third.bin') -Target ($source+'\a.bin') | Out-Null;
 [IO.File]::WriteAllBytes($source+'\é漢𝄞.bin',[byte[]](0,255,1,254));
 [IO.File]::WriteAllBytes($source+'\empty.txt',[byte[]]@());
 (Get-Item -LiteralPath ($source+'\a.bin')).Attributes='ReadOnly,Archive';
 (Get-Item -LiteralPath ($source+'\é漢𝄞.bin') -Force).Attributes='Hidden,Archive';
 $stamp=[DateTime]::Parse('2010-02-03T04:05:06Z').ToUniversalTime();
 @(Get-Item -LiteralPath $source -Force)+@(Get-ChildItem -LiteralPath $source -Force -Recurse) | ForEach-Object {$_.CreationTimeUtc=$stamp; $_.LastWriteTimeUtc=$stamp; $_.LastAccessTimeUtc=$stamp};
}
[Environment]::OSVersion.VersionString
'''
    source_suffix = r'\source-é漢'
    if args.protected_acl:
        source_suffix = r'\source-protected-é漢'
        setup_command = setup_command.replace("$source=$root+'\\source-é漢';", "$source=$root+'\\source-protected-é漢';")
        acl_setup = r'''
 $acl=New-Object Security.AccessControl.DirectorySecurity;
 $acl.SetSecurityDescriptorSddlForm('O:SYG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;BU)');
 Set-Acl -LiteralPath $source -AclObject $acl;
 $fileAcl=New-Object Security.AccessControl.FileSecurity;
 $fileAcl.SetSecurityDescriptorSddlForm('O:BAG:SYD:P(D;;0x2;;;BU)(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)');
 Set-Acl -LiteralPath ($source+'\a.bin') -AclObject $fileAcl;
'''
        setup_command = setup_command.replace(" $stamp=[DateTime]", acl_setup + " $stamp=[DateTime]")
    setup = guest.powershell(setup_command)
    if setup['exit'] != 0:
        raise RuntimeError(setup)
    source = root + source_suffix
    fixture_manifest_command = "$r='" + source + "'; @(Get-Item -LiteralPath $r -Force)+@(Get-ChildItem -LiteralPath $r -Force -Recurse) | ForEach-Object { [PSCustomObject]@{Path=$_.FullName;Attrs=[int]$_.Attributes;Creation=$_.CreationTimeUtc.Ticks;Write=$_.LastWriteTimeUtc.Ticks;Hash=$(if(-not $_.PSIsContainer){(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash})} } | ConvertTo-Json -Compress"
    before = guest.powershell(fixture_manifest_command)
    assert before['exit'] == 0
    binary = args.dll.read_bytes()
    caller = args.probe.read_bytes()
    run_root = root + '\\' + args.implementation
    assert guest.powershell("New-Item -ItemType Directory -Force '" + run_root + "' | Out-Null")['exit'] == 0
    guest.put(run_root + r'\wim.dll', binary)
    guest.put(run_root + r'\probe.exe', caller)
    if args.windows_reader:
        original_dll = pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll').read_bytes()
        apply_caller = pathlib.Path('target/windows-abi-probe/probe-windows-independent-apply.exe').read_bytes()
        guest.put(run_root + r'\reader.dll', original_dll)
        guest.put(run_root + r'\reader.exe', apply_caller)
    config = root + r'\capture.ini'
    guest.put(config, b'[ExclusionList]\r\n\\link.bin\r\n')
    malformed = root + r'\malformed.ini'
    guest.put(malformed, b'[Unrecognized]\r\nx\r\n')
    cases = [('plain-no-acls', 0x20, '-', 0, 0), ('verbose', 0x24, '-', 0, 0),
             ('default-acls', 0, '-', 0, 0), ('strict-acls', 0x40, '-', 0, 0),
             ('no-and-strict-acls', 0x60, '-', 0, 0), ('unix-data', 0x30, '-', 0, 0),
             ('rpfix', 0x120, '-', 0, 0), ('norpfix', 0x220, '-', 0, 0),
             ('both-rpfix', 0x320, '-', 0, 0), ('unknown-high', 0x40000020, '-', 0, 0),
             ('ntfs', 0x21, '-', 0, 0), ('dereference', 0x22, '-', 0, 0),
             ('boot', 0x28, '-', 0, 0), ('config', 0x24, config, 0, 0),
             ('winconfig-explicit', 0x824, config, 0, 0), ('malformed-config', 0x24, malformed, 0, 0),
             ('missing-config', 0x24, root + r'\absent.ini', 0, 0),
             ('cancel-begin', 0x24, '-', 9, 1), ('cancel-dentry', 0x24, '-', 10, 1),
             ('cancel-end', 0x24, '-', 11, 1), ('invalid-begin', 0x24, '-', 9, 2)]
    if args.case:
        cases = [case for case in cases if case[0] == args.case]
        assert cases, 'unknown case'
    observations = []
    applied = None
    for name, flags, cfg, stop, status in cases:
        output = run_root + r'\capture-é漢.wim' if name == args.write_case else '-'
        arguments = [run_root + r'\wim.dll', source, output, str(flags), cfg, str(stop), str(status)]
        if args.init_flags is not None:
            arguments.append(str(args.init_flags))
        result = guest.execute(run_root + r'\probe.exe', arguments)
        observations.append({'case': name, 'flags': flags, 'result': result})
        if name == args.write_case and 'write 0' in result['stdout']:
            data = guest.get(output)
            lookup_size = struct.unpack_from('<Q', data, 48)[0] & ((1 << 56) - 1)
            lookup_offset = struct.unpack_from('<Q', data, 56)[0]
            metadata = None
            for offset in range(lookup_offset, lookup_offset + lookup_size, 50):
                stored = struct.unpack_from('<Q', data, offset)[0]
                if stored >> 56 & 2:
                    assert not stored >> 56 & 4, 'NO compression probe metadata must be plain'
                    resource_offset = struct.unpack_from('<Q', data, offset + 8)[0]
                    payload = data[resource_offset:resource_offset + (stored & ((1 << 56) - 1))]
                    root_offset = (struct.unpack_from('<I', payload)[0] + 7) & ~7
                    short_len, name_len = struct.unpack_from('<HH', payload, root_offset + 98)
                    metadata = {'size': len(payload), 'root_name_bytes': name_len, 'root_dos_bytes': short_len,
                                'sha1_matches_lookup': hashlib.sha1(payload).digest() == data[offset + 30:offset + 50]}
                    break
            assert metadata is not None
            with tempfile.TemporaryDirectory(prefix='windows-capture-independent-') as temp:
                path = pathlib.Path(temp) / 'capture.wim'
                path.write_bytes(data)
                env = {**os.environ, 'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
                verify = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex', 'verify', str(path)], env=env, capture_output=True, text=True)
                target = pathlib.Path(temp) / 'apply'
                apply = subprocess.run(['/tmp/wimlib-native-oracle/wimlib-imagex', 'apply', str(path), '1', str(target)], env=env, capture_output=True, text=True)
                entries = []
                for entry in sorted(target.rglob('*')):
                    entries.append({'path': str(entry.relative_to(target)), 'directory': entry.is_dir(),
                                    'hash': hashlib.sha256(entry.read_bytes()).hexdigest() if entry.is_file() else None,
                                    'links': entry.stat().st_nlink})
                applied = {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'verify_exit': verify.returncode,
                           'apply_exit': apply.returncode, 'entries': entries, 'verify_stderr': verify.stderr, 'apply_stderr': apply.stderr}
                applied['metadata'] = metadata
                applied['root_name_valid'] = (metadata['root_name_bytes'] == metadata['root_dos_bytes'] == 0
                                             and 'root directory has a nonempty name' not in verify.stderr + apply.stderr)
                assert verify.returncode == apply.returncode == 0
            if args.windows_reader:
                target = run_root + '\\apply-' + str(time.time_ns())
                windows_apply = guest.execute(run_root + r'\reader.exe', [run_root + r'\reader.dll', output, target, '0'])
                manifest = guest.powershell("$r='" + target + "'; @(Get-Item -LiteralPath $r -Force)+@(Get-ChildItem -LiteralPath $r -Recurse -Force) | ForEach-Object {[PSCustomObject]@{Path=$_.FullName.Substring($r.Length);Attrs=[int]$_.Attributes;SDDL=(Get-Acl -LiteralPath $_.FullName).Sddl;Hash=$(if(-not $_.PSIsContainer){(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash})}} | ConvertTo-Json -Compress")
                assert windows_apply['exit'] == 0, windows_apply
                assert manifest['exit'] == 0, manifest
                applied['windows_reader'] = {'result': windows_apply, 'manifest': json.loads(manifest['stdout']),
                                             'dll_sha256': hashlib.sha256(original_dll).hexdigest(),
                                             'caller_sha256': hashlib.sha256(apply_caller).hexdigest()}
    after = guest.powershell(fixture_manifest_command)
    preserved = before['stdout'] == after['stdout']
    result = {'scope': 'Actual Windows capture contracts; basic NO_ACLS fixture and explicit full-feature gates',
              'implementation': args.implementation, 'guest': setup['stdout'].strip(),
              'dll_sha256': hashlib.sha256(binary).hexdigest(), 'caller_sha256': hashlib.sha256(caller).hexdigest(),
              'source_preserved': preserved, 'source_manifest': json.loads(before['stdout']),
              'source_manifest_after': json.loads(after['stdout']), 'cases': observations, 'independent_reader': applied}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        selected = {case['case']: case for case in baseline['cases']}
        def behavior(record):
            return {key: value for key, value in record.items() if key != 'stdout_base64'}
        def equivalent(a, b):
            left, right = behavior(a), behavior(b)
            original_lines = left['stdout'].splitlines()
            native_lines = right['stdout'].splitlines()
            if len(original_lines) != len(native_lines):
                return False
            normalized_original = []
            normalized_native = []
            for original_line, native_line in zip(original_lines, native_lines):
                # NTFS updates live access timestamps on actual payload reads and
                # directory enumeration. Keep fixed historical timestamps exact;
                # only allow corresponding live values within one day of oracle.
                if original_line.startswith('node ') and native_line.startswith('node '):
                    old, new = original_line.rsplit(' ', 1), native_line.rsplit(' ', 1)
                    old_seconds = int(old[1].split(':')[0])
                    new_seconds = int(new[1].split(':')[0])
                    if old_seconds >= 1577836800 and abs(new_seconds - old_seconds) <= 86400:
                        original_line = old[0] + ' <live-NTFS-access-time>'
                        native_line = new[0] + ' <live-NTFS-access-time>'
                normalized_original.append(original_line)
                normalized_native.append(native_line)
            left['stdout'] = '\n'.join(normalized_original)
            right['stdout'] = '\n'.join(normalized_native)
            return left == right
        result['differences'] = [{'case': a['case'], 'original': a['result'], 'native': b['result']}
                                 for b in observations for a in [selected[b['case']]]
                                 if not equivalent(a['result'], b['result'])]
        result['normalization'] = 'Only corresponding live NTFS access timestamps (>=2020, within24h oracle); fixed historical access times, all creation/write times and other fields exact'
        if applied is not None and baseline['independent_reader'] is not None:
            result['independent_reader_matches'] = (
                applied['root_name_valid'] and applied['entries'] == baseline['independent_reader']['entries']
                and applied['size'] == baseline['independent_reader']['size']
                and '\n'.join(line for line in applied['apply_stderr'].splitlines() if line.strip())
                == '\n'.join(line for line in baseline['independent_reader']['apply_stderr'].splitlines() if line.strip()))
            if 'windows_reader' in applied and 'windows_reader' in baseline['independent_reader']:
                result['windows_reader_matches'] = (
                    applied['windows_reader']['result']['exit'] == 0
                    and applied['windows_reader']['manifest'] == baseline['independent_reader']['windows_reader']['manifest'])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'source_preserved': preserved, 'independent_reader': applied,
                      'differences': len(result.get('differences', []))}, indent=2))


if __name__ == '__main__':
    main()
