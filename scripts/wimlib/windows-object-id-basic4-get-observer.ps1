# GET-only existing Basic4 scratch challenge observer; no creators/setters.
param([Parameter(Mandatory=$true)][string]$InputJson,[Parameter(Mandatory=$true)][string]$ExpectedInputSHA256,[Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
if((Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash.ToLowerInvariant()-cne$ExpectedInputSHA256.ToLowerInvariant()){throw 'Input SHA mismatch'}
$x=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($x.schema-ne1-or$x.mode-cne'GET-only-objectid-basic4-existing'-or$x.rows.Count-ne6-or$x.helper_sha256-cne(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()-or$x.scratch_root-notmatch'^[A-Za-z]:\\objectid-basic4-scratch-[0-9a-f]{32}$'){throw 'Bound observer input mismatch'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class Basic4GetObserver {
 [StructLayout(LayoutKind.Sequential)] struct Luid{public uint low;public int high;}
 [StructLayout(LayoutKind.Sequential)] struct Priv{public uint count;public Luid luid;public uint attributes;}
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint access,out IntPtr t);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid id);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr t,bool disable,ref Priv p,uint size,IntPtr old,IntPtr needed);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint access,uint share,IntPtr sd,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint code,IntPtr input,uint length,byte[] output,uint size,out uint returned,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle h,int kind,byte[] b,uint size);
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] b,uint size,out uint needed);
 static void Error(){throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}
 public static void Enable(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string name in new[]{"SeBackupPrivilege","SeSecurityPrivilege"}){var p=new Priv{count=1,attributes=2};if(!LookupPrivilegeValue(null,name,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();int error=Marshal.GetLastWin32Error();if(error!=0)throw new System.ComponentModel.Win32Exception(error);}}finally{CloseHandle(t);}}
 public sealed class Result{public byte[] BasicBefore,BasicAfter,Identity,Security,ObjectId;public int ObjectStatus;}
 public static Result Observe(string path){using(var h=CreateFileW(path,0x01020080,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){if(h.IsInvalid)Error();var r=new Result{BasicBefore=new byte[40],BasicAfter=new byte[40],Identity=new byte[24],Security=new byte[1048576],ObjectId=new byte[64]};if(!GetFileInformationByHandleEx(h,0,r.BasicBefore,40)||!GetFileInformationByHandleEx(h,18,r.Identity,24))Error();if((BitConverter.ToUInt32(r.BasicBefore,32)&0x400)!=0)throw new Exception("Reparse endpoint forbidden");uint n;if(!DeviceIoControl(h,0x9009c,IntPtr.Zero,0,r.ObjectId,64,out n,IntPtr.Zero)){r.ObjectStatus=Marshal.GetLastWin32Error();r.ObjectId=new byte[0];}else if(n!=64)throw new Exception("Full64 required");int status=NtQuerySecurityObject(h,0x1ff,r.Security,(uint)r.Security.Length,out n);if(status<0||n<20||n>r.Security.Length)throw new Exception("Security query "+status);Array.Resize(ref r.Security,(int)n);if(!GetFileInformationByHandleEx(h,0,r.BasicAfter,40))Error();return r;}}
}
'@
function Hex([byte[]]$b){[BitConverter]::ToString($b).Replace('-','').ToLowerInvariant()}
function Decode([string]$s){if($s-cnotmatch'^(?:[0-9a-f]{4})+$'){throw 'Raw UTF16 bounds'};[char[]]$u=for($i=0;$i-lt$s.Length;$i+=4){[char]([Convert]::ToByte($s.Substring($i,2),16)+256*[Convert]::ToByte($s.Substring($i+2,2),16))};$p=[string]::new($u);if(-not$p.StartsWith($x.scratch_root+'\',[StringComparison]::Ordinal)-or$p.Contains('/')-or$p.Contains([string][char]0)-or$p.Substring(2).Contains(':')-or@($p.Substring(3).Split('\')|Where-Object{$_-eq''-or$_-eq'.'-or$_-eq'..'}).Count){throw 'Selector escapes bound existing root'};return '\\?\'+$p}
function Row($r){return @{basic36=(Hex ([byte[]]$r.BasicBefore[0..35]));after_get_basic36=(Hex ([byte[]]$r.BasicAfter[0..35]));basic40=(Hex $r.BasicBefore);after_get_basic40=(Hex $r.BasicAfter);volume_serial=[BitConverter]::ToUInt64($r.Identity,0);file_id_info24=(Hex $r.Identity);security_raw=(Hex $r.Security);object_id_full64=(Hex $r.ObjectId);object_status=$r.ObjectStatus}}
function CompareExpected($actual, $expected, [UInt64]$volumeSerial) {
    foreach ($field in @('basic36', 'file_id_info24', 'security_raw', 'object_id_full64')) {
        if ($null -eq $actual.$field -or $null -eq $expected.$field -or $actual.$field -cne $expected.$field) { return $false }
    }
    if ($actual.basic36 -cne $actual.after_get_basic36 -or $actual.volume_serial -ne $volumeSerial) { return $false }
    if ($expected.object_id_full64) { return $actual.object_status -eq 0 }
    # Only these observed Windows absence statuses are accepted, with an empty
    # source-bound expected ObjectID and exact identity/security/basic metadata.
    return $actual.object_status -in @(2, 4312)
}
[Basic4GetObserver]::Enable();$rows=@()
foreach($expected in $x.rows){$row=@{path_utf16_le=$expected.path_utf16_le;passed=$false;error=$null};try{$p=Decode $expected.path_utf16_le;$row.first=Row ([Basic4GetObserver]::Observe($p));$row.reopened=Row ([Basic4GetObserver]::Observe($p));$row.passed=(CompareExpected $row.first $expected ([UInt64]$x.volume_serial))-and(CompareExpected $row.reopened $expected ([UInt64]$x.volume_serial))}catch{$row.error=$_.Exception.Message};$rows+=$row}
$service=Get-CimInstance Win32_Service -Filter "Name='TrkWks'";$trackingNormal=$service.State-ceq'Running'-and$service.StartMode-ceq'Auto'
$report=@{schema=1;mode=$x.mode;helper_sha256=$x.helper_sha256;input_sha256=$ExpectedInputSHA256;source_report_sha256=$x.source_report_sha256;frozen_before_reboot_sha256=$x.frozen_before_reboot_sha256;scope='Actual observations on new reboot child; parent hash is provenance anchor, not actual live child physical hash';last_boot_utc=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');kernel=[Environment]::OSVersion.Version.ToString();tracking_service_normal=$trackingNormal;trk_wks=($service|Select-Object State,StartMode,ProcessId);rows=$rows;all_passed=(@($rows|Where-Object{-not$_.passed}).Count-eq0-and$trackingNormal);production_executable=$false}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 30));$writer.Flush()}finally{$stream.Dispose()};if(-not$report.all_passed){throw 'Basic4 GET-only normalboot gate failed; evidence preserved'}
