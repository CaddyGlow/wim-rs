#!/usr/bin/env python3
"""Validate WIM EFS, binary EAs and object IDs against original wimlib on NTFS."""
import argparse
import hashlib
import json
from pathlib import Path
import time
from windows_guest import WindowsGuest

HELPER = r'''Add-Type -TypeDefinition '
using System;using System.IO;using System.Runtime.InteropServices;using System.Security.Cryptography;using Microsoft.Win32.SafeHandles;
public class NtfsProbe {
 [StructLayout(LayoutKind.Sequential)] struct Status {public IntPtr code;public UIntPtr bytes;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sd,uint d,uint f,IntPtr t);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint il,byte[] o,uint ol,out uint r,IntPtr ov);
 [DllImport("ntdll.dll")] static extern int NtSetEaFile(SafeFileHandle h,out Status s,byte[] b,uint l);
 [DllImport("ntdll.dll")] static extern int NtQueryEaFile(SafeFileHandle h,out Status s,byte[] b,uint l,byte single,IntPtr list,uint ll,IntPtr index,byte restart);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode)] static extern uint OpenEncryptedFileRaw(string p,uint f,out IntPtr c);
 delegate uint Export(IntPtr data,IntPtr context,uint length);
 [DllImport("advapi32.dll")] static extern uint ReadEncryptedFileRaw(Export e,IntPtr p,IntPtr c);
 [DllImport("advapi32.dll")] static extern void CloseEncryptedFileRaw(IntPtr c);
 static SafeFileHandle Open(string p,bool write){var h=CreateFile(p,write?0xC0000000u:0x80000000u,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero);if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return h;}
 public static void SetEA(string p){byte[] b={0,0,0,0,128,7,4,0,84,101,115,116,46,69,65,0,0,255,18,0};Status s;using(var h=Open(p,true)){int e=NtSetEaFile(h,out s,b,(uint)b.Length);if(e<0)throw new Exception("set EA "+e);}}
 public static string EA(string p){byte[] b=new byte[65536];Status s;using(var h=Open(p,false)){int e=NtQueryEaFile(h,out s,b,(uint)b.Length,0,IntPtr.Zero,0,IntPtr.Zero,1);if(e==unchecked((int)0xc0000052)||e==unchecked((int)0x80000015))return "";if(e<0)throw new Exception("get EA "+e);return BitConverter.ToString(b,0,(int)s.bytes.ToUInt64());}}
 public static string ObjectID(string p,bool create){byte[] b=new byte[64];uint n;using(var h=Open(p,create)){if(!DeviceIoControl(h,create?0x900c0u:0x9009cu,IntPtr.Zero,0,b,64,out n,IntPtr.Zero)){int e=Marshal.GetLastWin32Error();if(e==4312||e==2)return "";throw new System.ComponentModel.Win32Exception(e);}return BitConverter.ToString(b,0,(int)n);}}
 public static void DeleteID(string p){uint n;using(var h=Open(p,true)){if(!DeviceIoControl(h,0x900a0,IntPtr.Zero,0,null,0,out n,IntPtr.Zero)){int e=Marshal.GetLastWin32Error();if(e!=4312&&e!=2)throw new System.ComponentModel.Win32Exception(e);}}}
 public static string RawHash(string p){IntPtr c;uint e=OpenEncryptedFileRaw(p,0,out c);if(e!=0)throw new Exception("EFS open "+e);try{using(var sha=SHA256.Create()){Export cb=delegate(IntPtr d,IntPtr x,uint n){byte[] b=new byte[n];Marshal.Copy(d,b,0,(int)n);sha.TransformBlock(b,0,b.Length,null,0);return 0;};e=ReadEncryptedFileRaw(cb,IntPtr.Zero,c);if(e!=0)throw new Exception("EFS export "+e);sha.TransformFinalBlock(new byte[0],0,0);return BitConverter.ToString(sha.Hash);}}finally{CloseEncryptedFileRaw(c);}}
 public static string Hash(string p){using(var h=Open(p,false))using(var f=new FileStream(h,FileAccess.Read))using(var sha=SHA256.Create()){return BitConverter.ToString(sha.ComputeHash(f));}}
}';
'''

def clean(result):
    result.pop('stdout_base64', None)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', required=True, type=Path)
    parser.add_argument('--original-dll', type=Path, default=Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--probe-dir', type=Path, default=Path('target/windows-abi-probe'))
    parser.add_argument('--output', type=Path, default=Path('docs/wimlib/evidence/windows-ntfs-metadata/differential.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-ntfs-' + str(time.time_ns())
    setup = clean(guest.powershell(HELPER + r'''
$ErrorActionPreference='Stop';$r='ROOT';$s=$r+'\source';
New-Item -ItemType Directory -Force ($s+'\ea-directory')|Out-Null;
[IO.File]::WriteAllBytes($s+'\ea-file',[byte[]](0,255,1,2,3));
[NtfsProbe]::SetEA($s+'\ea-file');[NtfsProbe]::SetEA($s+'\ea-directory');[NtfsProbe]::SetEA($s);
[IO.File]::WriteAllText($s+'\object-file','object payload');[NtfsProbe]::ObjectID($s+'\object-file',$true)|Out-Null;
[NtfsProbe]::ObjectID($s+'\ea-directory',$true)|Out-Null;
[IO.File]::WriteAllText($s+'\encrypted','encrypted payload');
& cipher.exe /e /a ($s+'\encrypted')|Out-Null;if($LASTEXITCODE){throw 'EFS file setup failed'};
New-Item -ItemType HardLink -Path ($s+'\encrypted-alias') -Target ($s+'\encrypted')|Out-Null;
New-Item -ItemType Directory ($s+'\encrypted-directory')|Out-Null;
& cipher.exe /e ($s+'\encrypted-directory')|Out-Null;if($LASTEXITCODE){throw 'EFS directory setup failed'};
[IO.File]::WriteAllText($s+'\encrypted-directory\child','encrypted child payload');
[IO.File]::WriteAllText($s+'\empty-encrypted','');& cipher.exe /e /a ($s+'\empty-encrypted')|Out-Null;if($LASTEXITCODE){throw 'empty EFS setup failed'};
[NtfsProbe]::SetEA($s+'\encrypted');[NtfsProbe]::SetEA($s+'\encrypted-directory');[NtfsProbe]::SetEA($s+'\empty-encrypted');
$fixed=[DateTime]::SpecifyKind([DateTime]::Parse('2010-01-02T03:04:05'),[DateTimeKind]::Utc);
@(Get-Item $s)+@(Get-ChildItem $s -Force -Recurse)|ForEach-Object{$_.CreationTimeUtc=$fixed;$_.LastWriteTimeUtc=$fixed};
[ordered]@{account=(& whoami);os=(Get-CimInstance Win32_OperatingSystem).Caption;filesystem=(Get-Volume -DriveLetter C).FileSystem}|ConvertTo-Json
'''.replace('ROOT', root)))
    assert setup['exit'] == 0, setup
    artifacts = {}
    for name, path in [('original.dll', args.original_dll), ('rust.dll', args.dll), ('capture.exe', args.probe_dir/'probe-windows-capture.exe'), ('extract.exe', args.probe_dir/'probe-windows-extract-paths.exe')]:
        data = path.read_bytes()
        guest.put(root+'\\'+name, data)
        artifacts[name] = hashlib.sha256(data).hexdigest()
    inventory_code = HELPER + r'''
$ErrorActionPreference='Stop';$r='TARGET';
@(Get-Item $r)+@(Get-ChildItem $r -Force -Recurse)|ForEach-Object{
$p=$_.FullName;$efs=([int]$_.Attributes -band 16384)-ne 0;
[PSCustomObject]@{Path=$p.Substring($r.Length);Directory=$_.PSIsContainer;Attributes=[int]$_.Attributes;Creation=$_.CreationTimeUtc.Ticks;Write=$_.LastWriteTimeUtc.Ticks;SDDL=(Get-Acl -LiteralPath $p).Sddl;EA=[NtfsProbe]::EA($p);ObjectID=[NtfsProbe]::ObjectID($p,$false);Encrypted=$efs;Hash=$(if(-not $_.PSIsContainer){[NtfsProbe]::Hash($p)});RawHash=$(if($efs){[NtfsProbe]::RawHash($p)})}
}|Sort-Object Path|ConvertTo-Json -Depth 4 -Compress
'''
    def inventory(target):
        result = clean(guest.powershell(inventory_code.replace('TARGET', target)))
        assert result['exit'] == 0, result
        return json.loads(result['stdout'])
    expected = inventory(root+r'\source')
    captures = {}
    inputs = {}
    for label in ['original', 'rust']:
        source = root+'\\'+label+'.wim'
        run = clean(guest.execute(root+r'\capture.exe', [root+'\\'+label+'.dll', root+r'\source', source, '0', '-', '-1', '0']))
        captures[label] = run
        assert run['exit'] == 0 and 'write 0' in run['stdout'], run
        inputs[label] = hashlib.sha256(guest.get(source)).hexdigest()
    # IDs are unique per volume: release only fixture IDs, keeping archives unchanged.
    release = clean(guest.powershell(HELPER + r"[NtfsProbe]::DeleteID('ROOT\source\object-file');[NtfsProbe]::DeleteID('ROOT\source\ea-directory');".replace('ROOT', root)))
    assert release['exit'] == 0, release
    cases = []
    for captured in ['original', 'rust']:
        for extracted in ['original', 'rust']:
            target = root+'\\'+captured+'-'+extracted
            source = root+'\\'+captured+'.wim'
            run = clean(guest.execute(root+r'\extract.exe', [root+'\\'+extracted+'.dll', source, target, '0']))
            case = {'captured': captured, 'extracted': extracted, 'result': run}
            if run['exit'] == 0:
                case['inventory'] = inventory(target)
                case['equal'] = case['inventory'] == expected
                cleanup = {'exit': 0} if captured == 'rust' and extracted == 'rust' else clean(guest.powershell(HELPER + "[NtfsProbe]::DeleteID('TARGET\\object-file');[NtfsProbe]::DeleteID('TARGET\\ea-directory');".replace('TARGET', target)))
                assert cleanup['exit'] == 0, cleanup
            else:
                case['equal'] = False
            cases.append(case)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.with_suffix('.partial.json').write_text(json.dumps(cases, indent=2)+'\n')
    for extracted in ['original', 'rust']:
        target = root+'\\collision-'+extracted
        run = clean(guest.execute(root+r'\extract.exe', [root+'\\'+extracted+'.dll', root+r'\original.wim', target, '0']))
        actual = inventory(target) if run['exit'] == 0 else []
        collision_expected = [dict(i, ObjectID='') if i['ObjectID'] else i for i in expected]
        cases.append({'captured': 'original', 'extracted': extracted+'-collision', 'result': run, 'inventory': actual, 'equal': actual == collision_expected})
    for extracted in ['original', 'rust']:
        target = root+'\\selected-'+extracted
        run = clean(guest.execute(root+r'\extract.exe', [root+'\\'+extracted+'.dll', root+r'\original.wim', target, '0', 'encrypted']))
        actual = inventory(target) if run['exit'] == 0 else []
        selected_expected = [i for i in expected if i['Path'] in ['', r'\encrypted']]
        cases.append({'captured': 'original', 'extracted': extracted+'-selected-efs', 'result': run, 'inventory': actual, 'equal': actual == selected_expected})
    for label, digest in inputs.items():
        assert hashlib.sha256(guest.get(root+'\\'+label+'.wim')).hexdigest() == digest
    evidence = {'environment': setup, 'artifacts': artifacts, 'expected': expected, 'captures': captures, 'input_sha256': inputs, 'source_wims_preserved': True, 'cases': cases}
    args.output.write_text(json.dumps(evidence, indent=2)+'\n')
    print(json.dumps({'cases': len(cases), 'differences': [c['captured']+'->'+c['extracted'] for c in cases if not c['equal']]}))
    return 0 if all(c['equal'] for c in cases) else 1

if __name__ == '__main__':
    raise SystemExit(main())
