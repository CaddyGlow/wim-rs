#!/usr/bin/env python3
"""Create an isolated >500-dentry/DOS-hardlink fixture with the original DLL."""
import argparse
import hashlib
import json
import pathlib
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dos-representative', action='store_true')
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-extract-extra-dos-20261003' if args.dos_representative else r'C:\wim-extract-extra-20261003'
    command = r'''
$r='C:\wim-extract-extra-20261003';$s=$r+'\source';
if(Test-Path -LiteralPath $s){throw 'fixture already exists; preserve it'};
New-Item -ItemType Directory -Path ($s+'\nested') -Force|Out-Null;
for($n=0;$n -lt 505;$n++){[IO.File]::WriteAllBytes(($s+('\empty-{0:D4}.bin' -f $n)),[byte[]]@())};
$a=$s+'\long-primary-file.bin';$b=$s+'\nested\long-alias-file.bin';
[IO.File]::WriteAllBytes($a,[Text.Encoding]::ASCII.GetBytes("hello`n"));
New-Item -ItemType HardLink -Path $b -Target $a|Out-Null;
$d=[DateTime]::SpecifyKind([DateTime]::Parse('2010-01-02T03:04:05'),[DateTimeKind]::Utc);
@(Get-Item $s)+@(Get-ChildItem $s -Recurse -Force)|ForEach-Object{$_.CreationTimeUtc=$d;$_.LastWriteTimeUtc=$d};
& fsutil.exe file setshortname $a 'PRIM~1.BIN'|Out-Null;$first=$LASTEXITCODE;
& fsutil.exe file setshortname $b 'ALIA~1.BIN'|Out-Null;$second=$LASTEXITCODE;
[PSCustomObject]@{Entries=(@(Get-Item $s)+@(Get-ChildItem $s -Recurse -Force)).Count;PrimaryShortStatus=$first;AliasShortStatus=$second}|ConvertTo-Json -Compress
'''
    if args.dos_representative:
        command = command.replace(r'C:\wim-extract-extra-20261003', root)
        command = command.replace("$s+'\\long-primary-file.bin'", "$s+'\\nested\\z-primary-with-dos.bin'")
        command = command.replace("$s+'\\nested\\long-alias-file.bin'", "$s+'\\a-alias-without-dos.bin'")
    setup = guest.powershell(command)
    assert setup['exit'] == 0, setup
    guest.put(root + r'\original.dll', pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll').read_bytes())
    caller = pathlib.Path('target/windows-abi-probe/probe-windows-capture.exe').read_bytes()
    guest.put(root + r'\capture.exe', caller)
    generation = guest.execute(root + r'\capture.exe', [root + r'\original.dll', root + r'\source',
        root + r'\original-extra.wim', '0', '-', '-1', '0'])
    assert generation['exit'] == 0 and not generation['stdout_truncated'] and 'write 0' in generation['stdout'], generation
    fixture = guest.get(root + r'\original-extra.wim')
    result = {'scope': 'Owned isolated source; original DLL creates genuine 509-dentry WIM and real DOS-named hardlinks',
              'setup': setup, 'generation': generation, 'input_path': root + r'\original-extra.wim',
              'input_size': len(fixture), 'input_sha256': hashlib.sha256(fixture).hexdigest(),
              'caller_sha256': hashlib.sha256(caller).hexdigest()}
    output = pathlib.Path('docs/wimlib/evidence/native-windows-extract/' +
                          ('extra-dos-fixture-original.json' if args.dos_representative else 'extra-fixture-original.json'))
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'setup': json.loads(setup['stdout']), 'input_size': len(fixture)}))


if __name__ == '__main__':
    main()
