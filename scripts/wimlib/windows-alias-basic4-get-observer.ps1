# GET-only existing Basic4 scratch challenge observer; no creators/setters.
param([Parameter(Mandatory=$true)][string]$InputJson,[Parameter(Mandatory=$true)][string]$ExpectedInputSHA256,[Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
if((Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash.ToLowerInvariant()-cne$ExpectedInputSHA256.ToLowerInvariant()){throw 'Input SHA mismatch'}
$x=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($x.schema-ne1-or$x.mode-cne'GET-only-alias-basic4-existing'-or$x.rows.Count-ne8-or$x.helper_sha256-cne(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()-or$x.scratch_root-notmatch'^[A-Za-z]:\\alias-basic4-cycle-[0-9a-f]{32}$'){throw 'Bound observer input mismatch'}
$driveLetter=$x.scratch_root.Substring(0,1);$partition=Get-Partition -DriveLetter $driveLetter;$disk=$partition|Get-Disk;$volume=Get-Volume -DriveLetter $driveLetter
if($x.disk_serial-cne'V14-ALIAS2-ONLY'-or$disk.SerialNumber.Trim()-cne$x.disk_serial-or$disk.IsBoot-or$disk.IsSystem-or$disk.PartitionStyle-cne'GPT'-or$volume.FileSystem-cne'NTFS'-or$x.scratch_root.Substring(0,2)-ieq$env:SystemDrive){throw 'Read-only scratch physical identity mismatch'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class AliasBasic4GetObserver {
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
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct FindData {
  public uint Attributes; public System.Runtime.InteropServices.ComTypes.FILETIME Creation,Access,Write;
  public uint SizeHigh,SizeLow,Reserved0,Reserved1;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string Name;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=14)] public string ShortName;
 }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr FindFirstFileW(string path,out FindData data);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool FindNextFileW(IntPtr h,out FindData data);
 [DllImport("kernel32.dll")] static extern bool FindClose(IntPtr h);
 public static FindData[] Enumerate(string parent) {
  FindData data;var h=FindFirstFileW(parent+"\\*",out data);if(h==new IntPtr(-1))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  var rows=new System.Collections.Generic.List<FindData>();
  try { do {if(data.Name!="."&&data.Name!="..")rows.Add(data);} while(FindNextFileW(h,out data));int status=Marshal.GetLastWin32Error();if(status!=18)throw new System.ComponentModel.Win32Exception(status); }finally{FindClose(h);}
  return rows.ToArray();
 }
}
'@
function Hex([byte[]]$b){[BitConverter]::ToString($b).Replace('-','').ToLowerInvariant()}
function Decode([string]$s){if($s-cnotmatch'^(?:[0-9a-f]{4})+$'){throw 'Raw UTF16 bounds'};[char[]]$u=for($i=0;$i-lt$s.Length;$i+=4){[char]([Convert]::ToByte($s.Substring($i,2),16)+256*[Convert]::ToByte($s.Substring($i+2,2),16))};$p=[string]::new($u);if(($p-cne$x.scratch_root-and-not$p.StartsWith($x.scratch_root+'\',[StringComparison]::Ordinal))-or$p.Contains('/')-or$p.Contains([string][char]0)-or$p.Substring(2).Contains(':')-or@($p.Substring(3).Split('\')|Where-Object{$_-eq''-or$_-eq'.'-or$_-eq'..'}).Count){throw 'Selector escapes bound existing root'};return '\\?\'+$p}
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
[AliasBasic4GetObserver]::Enable();$rows=@();$errors=@()
$rootRaw=Hex ([Text.Encoding]::Unicode.GetBytes($x.scratch_root));$rootExpected=@($x.rows|Where-Object{$_.path_utf16_le-ceq$rootRaw});if($rootExpected.Count-ne1){throw 'Exactly one bound root observation required'}
$rootPreflight=Row ([AliasBasic4GetObserver]::Observe(('\\?\'+$x.scratch_root)));if(-not(CompareExpected $rootPreflight $rootExpected[0] ([UInt64]$x.volume_serial))){throw 'Existing own root identity/metadata preflight failed'}
foreach($expected in $x.rows){$row=@{path_utf16_le=$expected.path_utf16_le;passed=$false;error=$null};try{$p=Decode $expected.path_utf16_le;$row.first=Row ([AliasBasic4GetObserver]::Observe($p))}catch{$row.error=$_.Exception.Message};$rows+=$row}
$directoryEntries=@();try{$directoryEntries=[AliasBasic4GetObserver]::Enumerate(('\\?\'+$x.scratch_root))}catch{$errors+=$_.Exception.Message}
# Final snapshots occur AFTER all initial opens (including opposite aliases) and directory enumeration.
$rootIndex=-1;for($i=0;$i-lt$x.rows.Count;$i++){if($x.rows[$i].path_utf16_le-ceq$rootRaw){$rootIndex=$i}};$finalOrder=@(0..($x.rows.Count-1)|Where-Object{$_-ne$rootIndex})+@($rootIndex)
foreach($i in $finalOrder){try{$p=Decode $x.rows[$i].path_utf16_le;$rows[$i].reopened=Row ([AliasBasic4GetObserver]::Observe($p));$rows[$i].passed=(CompareExpected $rows[$i].first $x.rows[$i] ([UInt64]$x.volume_serial))-and(CompareExpected $rows[$i].reopened $x.rows[$i] ([UInt64]$x.volume_serial))}catch{$rows[$i].error=$_.Exception.Message}}
$directoryExact=$directoryEntries.Count-eq$x.expected_directory_entries.Count
foreach($expected in $x.expected_directory_entries){$matches=@($directoryEntries|Where-Object{$_.Name-ceq$expected.name});if($matches.Count-ne1-or$matches[0].ShortName-cne$expected.short_name-or$matches[0].Attributes-ne$expected.attributes){$directoryExact=$false}}
$service=Get-CimInstance Win32_Service -Filter "Name='TrkWks'";$trackingNormal=$service.State-ceq'Running'-and$service.StartMode-ceq'Auto'
$report=@{schema=1;mode=$x.mode;helper_sha256=$x.helper_sha256;input_sha256=$ExpectedInputSHA256;source_report_sha256=$x.source_report_sha256;frozen_apply_sha256=$x.frozen_apply_sha256;native_apply_review_sha256=$x.native_apply_review_sha256;scope='GET-only observations on NEW reboot child; frozen parent digest is provenance anchor, not live child physical hash. Existing two MFTDOS cachedChangeTime failures remain; runtime API snapshots cannot prove cachedFN fidelity.';last_boot_utc=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');kernel=[Environment]::OSVersion.Version.ToString();disk_serial=$disk.SerialNumber;volume_unique_id=$volume.UniqueId;tracking_service_normal=$trackingNormal;trk_wks=($service|Select-Object State,StartMode,ProcessId);root_preflight=$rootPreflight;rows=$rows;directory_entries=$directoryEntries;directory_exact=$directoryExact;errors=$errors;all_observations_stable=(@($rows|Where-Object{-not$_.passed}).Count-eq0-and$directoryExact-and$errors.Count-eq0-and$trackingNormal);production_executable=$false;strict_cached_MFT_filename_gate_passed=$false}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 35));$writer.Flush()}finally{$stream.Dispose()};if(-not$report.all_observations_stable){throw 'Alias GET-only normalboot observation gate failed; evidence preserved'}
