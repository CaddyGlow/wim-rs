#!/usr/bin/env python3
"""Compare native capture metadata with upstream capture and independent NTFS reads."""
import argparse
import hashlib
import json
from pathlib import Path
import time

from windows_guest import WindowsGuest


HELPER = r"""
Add-Type -TypeDefinition '
using System;using System.IO;using System.Collections.Generic;using System.Runtime.InteropServices;using System.Security.Cryptography;using Microsoft.Win32.SafeHandles;
public class MetadataProbe {
 [StructLayout(LayoutKind.Sequential)] struct Status {public IntPtr code;public UIntPtr bytes;}
 [StructLayout(LayoutKind.Sequential)] struct Luid {public uint low;public int high;}
 [StructLayout(LayoutKind.Sequential)] struct Privilege {public uint count;public Luid luid;public uint attributes;}
 [StructLayout(LayoutKind.Sequential)] struct Info {public uint attrs;public uint cl,ch,al,ah,wl,wh,volume,sh,sl,links,ih,il;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sd,uint d,uint f,IntPtr t);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info i);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr p);
 [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint a,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid l);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr t,bool disable,ref Privilege p,uint length,IntPtr old,IntPtr needed);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool ConvertSecurityDescriptorToStringSecurityDescriptor(byte[] sd,uint rev,uint flags,out IntPtr text,out uint size);
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] sd,uint length,out uint needed);
 [DllImport("ntdll.dll")] static extern int NtQueryInformationFile(SafeFileHandle h,out Status s,byte[] b,uint l,uint kind);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint il,byte[] o,uint ol,out uint r,IntPtr ov);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(string s,uint rev,out IntPtr sd,out uint size);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool SetKernelObjectSecurity(SafeFileHandle h,uint info,IntPtr sd);
 public static void SetSecurity(string p,string s){IntPtr sd;uint size;if(!ConvertStringSecurityDescriptorToSecurityDescriptor(s,1,out sd,out size))Error();try{using(var h=Open(p,0x010c0000,3)){if(!SetKernelObjectSecurity(h,1|2|4|8|16,sd))Error();}}finally{LocalFree(sd);}}
 static void Error(){throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}
 public static void EnablePrivileges(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string n in new[]{"SeBackupPrivilege","SeRestorePrivilege","SeSecurityPrivilege"}){Privilege p=new Privilege();p.count=1;p.attributes=2;if(!LookupPrivilegeValue(null,n,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();if(Marshal.GetLastWin32Error()==1300)throw new Exception("missing privilege "+n);}}finally{CloseHandle(t);}}
 static SafeFileHandle Open(string p,uint access,uint creation){var h=CreateFile(p,access,7,IntPtr.Zero,creation,0x02200000,IntPtr.Zero);if(h.IsInvalid)Error();return h;}
 public static void WriteStream(string p,string name,byte[] data){using(var h=Open(p+":"+name,0x40000000,2))using(var f=new FileStream(h,FileAccess.Write)){f.Write(data,0,data.Length);}}
 public static string Security(string p){using(var h=Open(p,0x01020080,3)){byte[] b=new byte[65536];uint needed;int e=NtQuerySecurityObject(h,1|2|4|8|16|0x10000,b,(uint)b.Length,out needed);if(e<0)throw new Exception("security "+e);IntPtr text;uint size;if(!ConvertSecurityDescriptorToStringSecurityDescriptor(b,1,1|2|4|8|16,out text,out size))Error();try{return Marshal.PtrToStringUni(text);}finally{LocalFree(text);}}}
 public static string ShortName(string p){using(var h=Open(p,0x80,3)){byte[] b=new byte[128];Status s;int e=NtQueryInformationFile(h,out s,b,128,21);if(e<0)return "";return System.Text.Encoding.Unicode.GetString(b,4,(int)BitConverter.ToUInt32(b,0));}}
 public static object[] Streams(string p){var list=new List<object>();using(var h=Open(p,0x80,3)){byte[] b=new byte[4096];Status s;int e;while((e=NtQueryInformationFile(h,out s,b,(uint)b.Length,22))<0){if(e!=unchecked((int)0x80000005)&&e!=unchecked((int)0xc0000023))throw new Exception("streams "+e);if(b.Length>=16777216)throw new Exception("streams limit");b=new byte[b.Length*2];}int n=(int)s.bytes.ToUInt64();for(int o=0;o<n;){int length=(int)BitConverter.ToUInt32(b,o+4);string raw=System.Text.Encoding.Unicode.GetString(b,o+24,length);long size=BitConverter.ToInt64(b,o+8);if(raw.EndsWith(":$DATA",StringComparison.Ordinal)){string name=raw.Substring(1,raw.Length-7);string hash="";if(size!=0){using(var data=Open(p+raw,0x80000000,3))using(var f=new FileStream(data,FileAccess.Read))using(var sha=SHA256.Create()){hash=BitConverter.ToString(sha.ComputeHash(f));}}list.Add(new StreamResult{Name=name,Length=size,Hash=hash});}uint next=BitConverter.ToUInt32(b,o);if(next==0)break;o+=(int)next;}}return list.ToArray();}
 public class StreamResult {public string Name;public long Length;public string Hash;}
 public static object Information(string p){using(var h=Open(p,0x80,3)){Info i;if(!GetFileInformationByHandle(h,out i))Error();return new {Attributes=i.attrs,Creation=((ulong)i.ch<<32)|i.cl,Access=((ulong)i.ah<<32)|i.al,Write=((ulong)i.wh<<32)|i.wl,Identity=i.volume.ToString("X8")+":"+i.ih.ToString("X8")+i.il.ToString("X8")};}}
 public static object Reparse(string p,string root){using(var h=Open(p,0x80,3)){byte[] b=new byte[16384];uint n;if(!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out n,IntPtr.Zero))Error();uint tag=BitConverter.ToUInt32(b,0);if(tag==0xa0000003||tag==0xa000000c){int start=tag==0xa000000c?20:16;string sub=System.Text.Encoding.Unicode.GetString(b,start+BitConverter.ToUInt16(b,8),BitConverter.ToUInt16(b,10));string print=System.Text.Encoding.Unicode.GetString(b,start+BitConverter.ToUInt16(b,12),BitConverter.ToUInt16(b,14));return new {Tag=tag,Sub=sub.Replace(root,"<ROOT>"),Print=print.Replace(root,"<ROOT>"),Flags=tag==0xa000000c?BitConverter.ToUInt32(b,16):0};}return new {Tag=tag,Bytes=BitConverter.ToString(b,0,(int)n)};}}
}';
[MetadataProbe]::EnablePrivileges();
"""

SETUP = r"""
$ErrorActionPreference='Stop';$r='ROOT';$s=$r+'\source';
fsutil.exe behavior set disablelastaccess 1|Out-Null;if($LASTEXITCODE){throw 'disable access updates failed'};
New-Item -ItemType Directory ($s+'\directory') -Force|Out-Null;
[IO.File]::WriteAllText($s+'\file','primary payload');
New-Item -ItemType HardLink -Path ($s+'\alias') -Target ($s+'\file')|Out-Null;
[MetadataProbe]::WriteStream($s+'\file','empty',[byte[]]@());
[MetadataProbe]::WriteStream($s+'\file','unicode-λ',[byte[]](0,255,1,2));
for($i=0;$i-lt 160;$i++){[MetadataProbe]::WriteStream($s+'\file',('stream-'+$i),[byte[]]($i));}
[MetadataProbe]::WriteStream($s,'root',[byte[]](1,2,3));
[MetadataProbe]::WriteStream($s+'\directory','directory',[byte[]](4,5,6));
[IO.File]::WriteAllText($s+'\directory\child','child payload');
[IO.File]::WriteAllText($r+'\outside','external payload');
cmd.exe /c ('mklink "'+$s+'\relative" "file"')|Out-Null;if($LASTEXITCODE){throw 'relative link failed'};
cmd.exe /c ('mklink "'+$s+'\absolute" "'+$s+'\file"')|Out-Null;if($LASTEXITCODE){throw 'absolute link failed'};
cmd.exe /c ('mklink /J "'+$s+'\junction" "'+$s+'\directory"')|Out-Null;if($LASTEXITCODE){throw 'junction failed'};
cmd.exe /c ('mklink "'+$s+'\external" "'+$r+'\outside"')|Out-Null;if($LASTEXITCODE){throw 'external link failed'};
cmd.exe /c ('mklink "'+$s+'\dangling" "'+$s+'\missing"')|Out-Null;if($LASTEXITCODE){throw 'dangling link failed'};
[MetadataProbe]::WriteStream($s+'\relative','link-only',[byte[]](9,8,7));
[MetadataProbe]::WriteStream($s+'\junction','junction-only',[byte[]](6,5,4));
[IO.File]::WriteAllBytes($s+'\compressed',[byte[]]::new(262144));
compact.exe /c /i /f ($s+'\compressed')|Out-Null;if($LASTEXITCODE){throw 'NTFS compression failed'};
[IO.File]::WriteAllBytes($s+'\wof',[byte[]]::new(262144));
compact.exe /c /i /f /exe:LZX ($s+'\wof')|Out-Null;if($LASTEXITCODE){throw 'WOF compression failed'};
$sparse=[IO.File]::Create($s+'\sparse');$sparse.SetLength(262144);$sparse.Position=131072;$sparse.WriteByte(17);$sparse.Dispose();
fsutil.exe sparse setflag ($s+'\sparse')|Out-Null;if($LASTEXITCODE){throw 'sparse flag failed'};
fsutil.exe sparse setrange ($s+'\sparse') 0 131072|Out-Null;if($LASTEXITCODE){throw 'sparse hole failed'};
[IO.File]::WriteAllText($s+'\long-file-name-for-short-name.txt','short name payload');
fsutil.exe file setshortname ($s+'\long-file-name-for-short-name.txt') SHORT1.TXT|Out-Null;if($LASTEXITCODE){throw 'short name setup failed'};
[IO.File]::WriteAllText($s+'\protected','backup-read payload');
$fixed=[DateTime]::Parse('2010-01-02T03:04:05Z').ToUniversalTime();
@(Get-Item $s -Force)+@(Get-ChildItem $s -Force -Recurse)|Where-Object{([int]$_.Attributes-band 1024)-eq 0}|ForEach-Object{$_.CreationTimeUtc=$fixed;$_.LastWriteTimeUtc=$fixed;$_.LastAccessTimeUtc=$fixed};
[MetadataProbe]::SetSecurity($s+'\file','O:SYG:BAD:P(D;;0x2;;;BU)(A;;FA;;;SY)(A;;FA;;;BA)S:P(AU;SAFA;FR;;;WD)(ML;;NW;;;LW)');
[MetadataProbe]::SetSecurity($s+'\protected','O:SYG:BAD:P(D;;FR;;;WD)(A;;FA;;;SY)');
(Get-Item ($s+'\long-file-name-for-short-name.txt')).Attributes='ReadOnly,Hidden,Archive';
Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'|Select-Object CurrentBuildNumber,UBR|ConvertTo-Json
"""

INVENTORY = r"""
$ErrorActionPreference='Stop';$r='ROOT';
$paths=New-Object 'Collections.Generic.List[string]';$paths.Add($r);
function Walk([string]$p){foreach($item in Get-ChildItem -LiteralPath $p -Force){$paths.Add($item.FullName);if($item.PSIsContainer-and (([int]$item.Attributes-band 1024)-eq 0)){Walk $item.FullName;}}};Walk $r;
@(foreach($p in $paths){$i=[MetadataProbe]::Information($p);[PSCustomObject]@{Path=$p.Substring($r.Length);Info=$i;Security=[MetadataProbe]::Security($p);Short=[MetadataProbe]::ShortName($p);Streams=@([MetadataProbe]::Streams($p)|Sort-Object Name);Reparse=$(if(($i.Attributes-band 1024)-ne 0){[MetadataProbe]::Reparse($p,'NORMALIZE_ROOT')})}})|ConvertTo-Json -Depth 8 -Compress
"""


def checked(result):
    if result['exit'] != 0 or result.get('stdout_truncated'):
        raise RuntimeError(result)
    result.pop('stdout_base64', None)
    return result


def canonical(items):
    groups = {}
    for item in items:
        identity = item['Info']['Identity']
        groups.setdefault(identity, []).append(item['Path'])
    for item in items:
        identity = item['Info'].pop('Identity')
        if item['Path'] == '':
            item['Short'] = ''  # An image root has no captured directory-entry name.
        item['Links'] = sorted(groups[identity])
    return sorted(items, key=lambda item: item['Path'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', required=True, type=Path)
    parser.add_argument('--original-dll', type=Path, default=Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--probe-dir', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-metadata-' + str(time.time_ns())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    evidence = {'fixture': root, 'cases': [], 'artifacts': {}}
    def save():
        args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    evidence['setup'] = guest.powershell(HELPER + SETUP.replace('ROOT', root))
    save()
    checked(evidence['setup'])
    for name, path in [('original.dll', args.original_dll), ('rust.dll', args.dll), ('capture.exe', args.probe_dir / 'probe-windows-capture.exe'), ('extract.exe', args.probe_dir / 'probe-windows-extract-paths.exe')]:
        data = path.read_bytes()
        guest.put(root + '\\' + name, data)
        evidence['artifacts'][name] = hashlib.sha256(data).hexdigest()
    def inventory(target, normalize=None):
        command = INVENTORY.replace('NORMALIZE_ROOT', normalize or target).replace("$r='ROOT'", "$r='" + target + "'")
        return canonical(json.loads(checked(guest.powershell(HELPER + command))['stdout']))
    snapshot = checked(guest.powershell(r"$ErrorActionPreference='Stop';$created=([wmiclass]'Win32_ShadowCopy').Create('C:\','ClientAccessible');if($created.ReturnValue){throw ('VSS create failed '+$created.ReturnValue)};Get-WmiObject Win32_ShadowCopy|Where-Object{$_.ID-eq $created.ShadowID}|Select-Object ID,DeviceObject|ConvertTo-Json"))
    evidence['snapshot'] = json.loads(snapshot['stdout'])
    source = evidence['snapshot']['DeviceObject'] + root[2:] + r'\source'
    evidence['expected'] = inventory(source, root + r'\source')
    save()
    for label in ['original', 'rust']:
        output = root + '\\' + label + '.wim'
        capture = checked(guest.execute(root + r'\capture.exe', [root + '\\' + label + '.dll', source, output, '64', '-', '-1', '0', '12']))
        evidence[label + '_capture'] = capture
        save()
        if 'add 0' not in capture['stdout'] or 'write 0' not in capture['stdout']:
            raise RuntimeError(capture)
        for apply in ['original', 'rust', 'dism']:
            target = root + '\\' + label + '-' + apply
            if apply == 'dism':
                checked(guest.powershell("New-Item -ItemType Directory '" + target + "'|Out-Null"))
                result = checked(guest.powershell("& dism.exe /Apply-Image '/ImageFile:" + output + "' /Index:1 '/ApplyDir:" + target + "' /CheckIntegrity '/LogPath:" + root + '\\dism-' + label + ".log';exit $LASTEXITCODE"))
            else:
                result = checked(guest.execute(root + r'\extract.exe', [root + '\\' + apply + '.dll', output, target, '128']))
            actual = inventory(target)
            expected_by_path = {item['Path']: item for item in evidence['expected']}
            actual_by_path = {item['Path']: item for item in actual}
            differences = [{'expected': expected_by_path.get(path), 'actual': actual_by_path.get(path)}
                           for path in sorted(expected_by_path.keys() | actual_by_path.keys())
                           if expected_by_path.get(path) != actual_by_path.get(path)]
            equal = actual == evidence['expected']
            evidence['cases'].append({'capture': label, 'apply': apply, 'result': result, 'inventory': actual, 'equal': equal, 'differences': differences})
            save()
    evidence['source_after'] = inventory(source, root + r'\source')
    evidence['source_preserved'] = evidence['source_after'] == evidence['expected']
    save()
    # Preserve raw DISM differences. Accept only independently reproduced
    # upstream behavior: redundant 8.3 aliases on links and null versus absent SACL.
    baseline = next(c for c in evidence['cases'] if c['capture'] == 'original' and c['apply'] == 'dism')
    allowed = True
    for diff in baseline['differences']:
        before, after = diff['expected'], diff['actual']
        if before is None or after is None:
            allowed = False
            continue
        keys = {key for key in before if before[key] != after[key]}
        if keys == {'Short'}:
            allowed &= (before['Info']['Attributes'] & 1024 != 0 and before['Short'] == before['Path'].lstrip('\\') and after['Short'] == '')
        elif keys == {'Security'}:
            allowed &= before['Security'].endswith('S:NO_ACCESS_CONTROL') and before['Security'].removesuffix('S:NO_ACCESS_CONTROL') == after['Security']
        else:
            allowed = False
    for case in evidence['cases']:
        case['passes'] = case['equal'] or (case['apply'] == 'dism' and allowed and case['differences'] == baseline['differences'])
    save()
    failures = [c['capture'] + '->' + c['apply'] for c in evidence['cases'] if not c['passes']]
    print(json.dumps({'cases': len(evidence['cases']), 'failures': failures, 'source_preserved': evidence['source_preserved']}))
    return 0 if not failures and evidence['source_preserved'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
