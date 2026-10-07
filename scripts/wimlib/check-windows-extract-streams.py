#!/usr/bin/env python3
"""Compare native-written NTFS streams and selected extraction in a disposable guest."""
import argparse
import hashlib
import json
import pathlib
import time
from windows_guest import WindowsGuest


STREAM_HELPER = r'''Add-Type -TypeDefinition '
using System;using System.IO;using System.Security.Cryptography;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public class NativeStream {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sd,uint d,uint f,IntPtr t);
 static FileStream Open(string p,bool write){var h=CreateFile(p,write?0x40000000u:0x80000000u,7,IntPtr.Zero,write?2u:3u,0x02000000,IntPtr.Zero);if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return new FileStream(h,write?FileAccess.Write:FileAccess.Read);}
 public static void Write(string p,byte[] b){using(var f=Open(p,true)){f.Write(b,0,b.Length);}}
 public static string Hash(string p){using(var f=Open(p,false))using(var h=SHA256.Create()){return BitConverter.ToString(h.ComputeHash(f)).Replace("-", "");}}
}';
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, required=True)
    parser.add_argument('--original-dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--probe', type=pathlib.Path, default=pathlib.Path('target/windows-abi-probe/probe-windows-extract-paths.exe'))
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/windows-extract-streams/differential.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-streams-' + str(time.time_ns())
    setup = guest.powershell(STREAM_HELPER + r'''
$ErrorActionPreference='Stop';$r='ROOT';$s=$r+'\source';
New-Item -ItemType Directory -Path ($s+'\nested') -Force|Out-Null;
[IO.File]::WriteAllBytes($s+'\nested\file-é漢',[Text.Encoding]::UTF8.GetBytes('main payload'));
[NativeStream]::Write($s+'\nested\file-é漢:ads',[Text.Encoding]::UTF8.GetBytes('named payload'));
[NativeStream]::Write($s+'\nested\file-é漢:empty',[byte[]]@());
[NativeStream]::Write($s+'\nested:directory-ads',[byte[]](1,2,3));
New-Item -ItemType HardLink -Path ($s+'\alias') -Target ($s+'\nested\file-é漢')|Out-Null;
[IO.File]::WriteAllBytes($s+'\compressed',[byte[]]::new(131072));
& compact.exe /c ($s+'\compressed')|Out-Null;if($LASTEXITCODE){throw 'compression failed'};
[IO.File]::WriteAllBytes($s+'\sparse',[byte[]]::new(131072));
& fsutil.exe sparse setflag ($s+'\sparse')|Out-Null;if($LASTEXITCODE){throw 'sparse failed'};
& fsutil.exe sparse setrange ($s+'\sparse') 0 131072|Out-Null;if($LASTEXITCODE){throw 'sparse range failed'};
cmd.exe /c ('mklink "'+$s+'\relative" "nested\file-é漢"')|Out-Null;if($LASTEXITCODE){throw 'relative link failed'};
cmd.exe /c ('mklink "'+$s+'\absolute" "'+$s+'\nested\file-é漢"')|Out-Null;if($LASTEXITCODE){throw 'absolute link failed'};
cmd.exe /c ('mklink /J "'+$s+'\junction" "'+$s+'\nested"')|Out-Null;if($LASTEXITCODE){throw 'junction failed'};
cmd.exe /c ('mklink "'+$s+'\dangling" "missing"')|Out-Null;if($LASTEXITCODE){throw 'dangling link failed'};
$d=[DateTime]::SpecifyKind([DateTime]::Parse('2010-01-02T03:04:05'),[DateTimeKind]::Utc);
@(Get-Item $s)+@(Get-ChildItem $s -Recurse -Force)|Where-Object{([int]$_.Attributes -band 1024)-eq 0}|ForEach-Object{$_.CreationTimeUtc=$d;$_.LastWriteTimeUtc=$d};
Write-Output 'prepared';
'''.replace('ROOT', root))
    assert setup['exit'] == 0, setup
    for label, path in [('original', args.original_dll), ('rust', args.dll), ('probe', args.probe), ('capture', pathlib.Path('target/windows-abi-probe/probe-windows-capture.exe'))]:
        guest.put(root + '\\' + label + ('.dll' if label in ['rust', 'original'] else '.exe'), path.read_bytes())
    source = root + r'\fixture.wim'
    capture = guest.execute(root + r'\capture.exe', [root + r'\original.dll', root + r'\source', source, '0', '-', '-1', '0'])
    assert capture['exit'] == 0 and 'write 0' in capture['stdout'], capture
    before = hashlib.sha256(guest.get(source)).hexdigest()
    cases = [ ('whole', 0, []), ('whole-no-fix', 0x200, []),
              ('preserve', 0, [r'nested\file-é漢']),
              ('flatten-hardlinks', 0x200000, [r'nested\file-é漢', 'alias']),
              ('glob', 0x40000, [r'nested\*']),
              ('flatten-directory', 0x200000, ['nested']),
              ('selected-links', 0x100, ['relative', 'absolute', 'junction', 'dangling']),
              ('missing', 0, ['absent']), ('strict-glob-missing', 0xc0000, [r'absent\*']) ]
    observations = []
    for name, flags, paths in cases:
        case = {'name': name, 'flags': flags, 'paths': paths}
        for label in ['original', 'rust']:
            target = root + '\\' + label + '-' + name
            run = guest.execute(root + r'\probe.exe', [root + '\\' + label + '.dll', source, target, str(flags)] + paths)
            inventory = guest.powershell(STREAM_HELPER + r'''
$ErrorActionPreference='Stop';$r='TARGET';
if(-not(Test-Path -LiteralPath $r)){Write-Output '[]';exit};
function Visit($p) {
 $i=Get-Item -LiteralPath $p -Force;
 $reparse=([int]$i.Attributes -band 1024)-ne 0;
 $streams=@();if(-not $reparse){
  foreach($st in @(Get-Item -LiteralPath $p -Stream * -ErrorAction SilentlyContinue)){
   if($st.Stream -eq ':$DATA' -and $i.PSIsContainer){continue};
   $sp=if($st.Stream -eq ':$DATA'){$p}else{$p+':'+$st.Stream};
   $hash=[NativeStream]::Hash($sp);
   $streams+=@{Name=$st.Stream;Size=$st.Length;Hash=$hash};
  }
 }
 $link=if($reparse){@($i.Target|ForEach-Object{$_.Replace($r,'<EXTRACTION-ROOT>')})}else{@()};
 [PSCustomObject]@{Path=$p.Substring($r.Length);Directory=$i.PSIsContainer;Attributes=[int]$i.Attributes;Creation=$(if(-not $reparse -and -not ($p -eq $r -and FLATTEN)){$i.CreationTimeUtc.Ticks});Write=$(if(-not $reparse -and -not ($p -eq $r -and FLATTEN)){$i.LastWriteTimeUtc.Ticks});SDDL=$(if(-not $reparse){(Get-Acl -LiteralPath $p).Sddl});Target=$link;Streams=@($streams|Sort-Object Name)};
 if($i.PSIsContainer -and -not $reparse){Get-ChildItem -LiteralPath $p -Force|ForEach-Object{Visit $_.FullName}}
}
@(Visit $r)|Sort-Object Path|ConvertTo-Json -Depth 6 -Compress
'''.replace('TARGET', target).replace('FLATTEN', '$true' if flags & 0x200000 else '$false'))
            assert inventory['exit'] == 0, inventory
            case[label] = {'result': run, 'inventory': json.loads(inventory['stdout'])}
        case['equal'] = case['original'] == case['rust']
        observations.append(case)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.with_suffix('.partial.json').write_text(json.dumps(observations, indent=2) + '\n')
    after = hashlib.sha256(guest.get(source)).hexdigest()
    assert before == after, 'input WIM changed'
    result = {'scope': 'Native-written WIM; real NTFS ADS, directory streams, empty streams, hardlinks, compression, sparse attributes, symlinks/junctions, selected paths, globbing and fixups',
              'input_sha256': before, 'source_preserved': True, 'setup': setup, 'capture': capture,
              'dll_sha256': hashlib.sha256(args.dll.read_bytes()).hexdigest(),
              'original_dll_sha256': hashlib.sha256(args.original_dll.read_bytes()).hexdigest(),
              'probe_sha256': hashlib.sha256(args.probe.read_bytes()).hexdigest(),
              'cases': observations, 'differences': [case['name'] for case in observations if not case['equal']]}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'differences': result['differences']}))


if __name__ == '__main__':
    main()
