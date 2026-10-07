# Two-phase NEW isolated alias two-cycle + own Basic4 restoration control.
# Preserve clean setup baseline before Apply on a NEW child; no installed setters.
param([Parameter(Mandatory=$true)][string]$InputJson,[Parameter(Mandatory=$true)][string]$ExpectedInputSHA256,[Parameter(Mandatory=$true)][string]$ScratchVolumeRoot,[Parameter(Mandatory=$true)][string]$ExpectedDiskSerial,[Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
if((Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash.ToLowerInvariant()-cne$ExpectedInputSHA256){throw 'Input digest mismatch'}
$x=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($x.schema-ne1-or$x.mode-cne'NEW-alias-two-cycle-basic4'-or$x.phase-notin@('Setup','Apply')-or$x.disk_serial-cne'V14-ALIAS2-ONLY'-or$x.disk_serial-cne$ExpectedDiskSerial-or$x.helper_sha256-cne(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()){throw 'Protocol/helper/serial binding mismatch'}
$drive=Get-Item -LiteralPath $ScratchVolumeRoot
if($drive.FullName-notmatch'^[A-Za-z]:\\$'-or$drive.FullName.Substring(0,2)-ieq$env:SystemDrive){throw 'Dedicated non-system root required'}
$partition=Get-Partition -DriveLetter $drive.FullName.Substring(0,1);$disk=$partition|Get-Disk;$volume=Get-Volume -DriveLetter $drive.FullName.Substring(0,1)
if($disk.IsBoot-or$disk.IsSystem-or$disk.SerialNumber.Trim()-cne$x.disk_serial-or$disk.PartitionStyle-cne'GPT'-or$volume.FileSystem-cne'NTFS'){throw 'Dedicated physical disk/NTFS mismatch'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.ComponentModel;
using Microsoft.Win32.SafeHandles;
public static class AliasBasic4Cycle {
 [StructLayout(LayoutKind.Sequential)] struct Luid{public uint low;public int high;}
 [StructLayout(LayoutKind.Sequential)] struct Priv{public uint count;public Luid luid;public uint attributes;}
 [StructLayout(LayoutKind.Sequential)] struct Status{public IntPtr status;public UIntPtr information;}
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint access,out IntPtr t);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid id);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr t,bool disable,ref Priv p,uint size,IntPtr old,IntPtr needed);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr sd,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint code,byte[] input,uint length,byte[] output,uint size,out uint returned,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle h,int kind,byte[] b,uint size);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateHardLinkW(string path,string existing,IntPtr security);
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] b,uint size,out uint needed);
 [DllImport("ntdll.dll")] static extern int NtSetInformationFile(SafeFileHandle h,out Status status,IntPtr buffer,uint length,int kind);
 static void Error(){throw new Win32Exception(Marshal.GetLastWin32Error());}
 public static void Enable(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string name in new[]{"SeBackupPrivilege","SeRestorePrivilege","SeSecurityPrivilege"}){var p=new Priv{count=1,attributes=2};if(!LookupPrivilegeValue(null,name,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();int error=Marshal.GetLastWin32Error();if(error!=0)throw new Win32Exception(error);}}finally{CloseHandle(t);}}
 static SafeFileHandle Open(string p,bool write,bool create=false){var h=CreateFileW(p,0x01020080u|(write?0x10100u:0),7,IntPtr.Zero,create?1u:3u,0x02200000,IntPtr.Zero);if(h.IsInvalid)Error();return h;}
 static byte[] Object(SafeFileHandle h,bool unused){var b=new byte[64];uint n;if(!DeviceIoControl(h,0x9009c,null,0,b,64,out n,IntPtr.Zero)){int e=Marshal.GetLastWin32Error();if(e==2||e==4312)return new byte[0];throw new Win32Exception(e);}if(n!=64)throw new Exception("Full64 required");return b;}
 public static void Link(string p,string existing){if(!CreateHardLinkW(p,existing,IntPtr.Zero))Error();}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetFileShortNameW(SafeFileHandle h,string name);
 public static int Alias(string path,string name,bool write){if(name!=""&&name!="ALPHAA~1.BIN"&&name!="ALPHAB~1.BIN")throw new ArgumentException("Unapproved scratch alias");using(var h=Open(path,write)){return SetFileShortNameW(h,name)?0:Marshal.GetLastWin32Error();}}
 public static int Basic(string path,byte[] before){using(var h=Open(path,true)){return Restore(h,before);}}
 static byte[][] Capture(SafeFileHandle h,bool objectId=true){var basic=new byte[40];var id=new byte[24];if(!GetFileInformationByHandleEx(h,0,basic,40)||!GetFileInformationByHandleEx(h,18,id,24))Error();if((BitConverter.ToUInt32(basic,32)&0x400)!=0)throw new Exception("Reparse forbidden");var sd=new byte[1048576];uint n;int status=NtQuerySecurityObject(h,0x1ff,sd,(uint)sd.Length,out n);if(status<0||n<20||n>sd.Length)throw new Exception("Security query "+status);Array.Resize(ref sd,(int)n);return new[]{basic,id,sd,objectId?Object(h,false):new byte[0]};}
 public static byte[][] Observe(string p,bool objectId=true){using(var h=Open(p,false)){return Capture(h,objectId);}}
 public static void ValidateBasic(byte[] b){if(b.Length!=40)throw new ArgumentException("FILE_BASIC_INFORMATION length");for(int o=0;o<32;o+=8){long v=BitConverter.ToInt64(b,o);if(v<=0)throw new ArgumentException("Literal zero/sentinel time cannot be restored exactly");}if(BitConverter.ToUInt32(b,32)==0)throw new ArgumentException("Zero attributes preserve current, not exact restore");}
 static int Restore(SafeFileHandle h,byte[] b){ValidateBasic(b);var copy=(byte[])b.Clone();Array.Clear(copy,36,4);var pinned=GCHandle.Alloc(copy,GCHandleType.Pinned);try{Status status;int result=NtSetInformationFile(h,out status,pinned.AddrOfPinnedObject(),40,4);return result;}finally{pinned.Free();}}
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
function Unhex([string]$s){if($s-cnotmatch'^(?:[0-9a-f]{2})+$'){throw 'Malformed bounded hex'};[byte[]]$b=for($i=0;$i-lt$s.Length;$i+=2){[Convert]::ToByte($s.Substring($i,2),16)};return ,$b}
function Observation([string]$path){$b=[AliasBasic4Cycle]::Observe($path);return @{basic40=(Hex $b[0]);basic36=(Hex ([byte[]]$b[0][0..35]));file_id_info24=(Hex $b[1]);security_raw=(Hex $b[2]);object_id_full64=(Hex $b[3])}}
function Same($a,$b){foreach($f in @('basic36','file_id_info24','security_raw','object_id_full64')){if($null-eq$a.$f-or$null-eq$b.$f-or$a.$f-cne$b.$f){return $false}};return $true}
function ValidateRoot([string]$p,[string]$driveRoot){if($p-cnotmatch'^[A-Za-z]:\\alias-basic4-cycle-[0-9a-f]{32}$'-or-not$p.StartsWith($driveRoot,[StringComparison]::Ordinal)){throw 'Unexpected scratch root'};return '\\?\'+$p}
function Snapshot($paths){$rows=@();foreach($p in $paths){$rows+=@{path_utf16_le=(Hex ([Text.Encoding]::Unicode.GetBytes($p)));observation=(Observation ('\\?\'+$p))}};return $rows}
function SnapshotSame($a,$b){if($a.Count-ne$b.Count){return $false};for($i=0;$i-lt$a.Count;$i++){if($a[$i].path_utf16_le-cne$b[$i].path_utf16_le-or-not(Same $a[$i].observation $b[$i].observation)){return $false}};return $true}
[AliasBasic4Cycle]::Enable();$actions=@();$report=@{schema=1;mode=$x.mode;phase=$x.phase;production_executable=$false;executed=$true;helper_sha256=$x.helper_sha256;input_sha256=$ExpectedInputSHA256;kernel=[Environment]::OSVersion.Version.ToString();privileges=(& whoami.exe /all|Out-String);disk_serial=$disk.SerialNumber;volume_unique_id=$volume.UniqueId;success=$false;offline_index_namespace_verified=$false;normal_reboot_verified=$false;scope='Two NEW ordinary files, same-parent hardlinks, sibling and parent only. Existing clear/swap feasibility previously observed; new gates are allfourtimes/fullSACL/INDEXnamespace+reboot. No production cycle coverage claim.'}
try{
 if($x.phase-ceq'Setup'){$root=Join-Path $drive.FullName ('alias-basic4-cycle-'+[Guid]::NewGuid().ToString('N'));if(Test-Path -LiteralPath $root){throw 'NEW root collision'};New-Item -ItemType Directory -Path $root|Out-Null}
 else{$root=[string]$x.scratch_root;ValidateRoot $root $drive.FullName|Out-Null;if($x.frozen_setup_disk_sha256-cnotmatch'^[0-9a-f]{64}$'-or$x.native_setup_index_review_sha256-cnotmatch'^[0-9a-f]{64}$'-or$x.expected_rows.Count-ne6){throw 'Frozen setup provenance/6selectors required'}}
 $nativeRoot=ValidateRoot $root $drive.FullName;$a=Join-Path $root 'LongAlphaPrimaryFile.bin';$b=Join-Path $root 'LongBetaPrimaryFile.bin';$ha=Join-Path $root 'AlphaSecondaryHardlink.bin';$hb=Join-Path $root 'BetaSecondaryHardlink.bin';$sibling=Join-Path $root 'UntouchedSibling.bin';$paths=@($a,$b,$ha,$hb,$sibling,$root)
 if($x.phase-ceq'Setup'){
  foreach($pair in @(@($a,'alpha-independent-payload'),@($b,'beta-independent-payload'),@($sibling,'unrelated-sibling'))){$stream=[IO.File]::Open($pair[0],[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);try{$bytes=[Text.Encoding]::UTF8.GetBytes($pair[1]);$stream.Write($bytes,0,$bytes.Length)}finally{$stream.Dispose()}}
  [AliasBasic4Cycle]::Link(('\\?\'+$ha),('\\?\'+$a));[AliasBasic4Cycle]::Link(('\\?\'+$hb),('\\?\'+$b))
  foreach($pair in @(@($a,'ALPHAA~1.BIN'),@($b,'ALPHAB~1.BIN'))){$status=[AliasBasic4Cycle]::Alias(('\\?\'+$pair[0]),$pair[1],$true);$actions+=@{phase='setup-short-name';path=$pair[0];name=$pair[1];status=$status};if($status-ne0){throw 'Setup alias assignment failed'}}
  $report.setup_rows=Snapshot $paths;$report.setup_directory_entries=[AliasBasic4Cycle]::Enumerate($nativeRoot);$report.success=$true
 }else{
  $before=Snapshot $paths;$report.before=$before;$report.directory_before=[AliasBasic4Cycle]::Enumerate($nativeRoot)
  if(-not(SnapshotSame $before $x.expected_rows)){throw 'Frozen baseline own-metadata precondition failed before mutation'}
  for($i=0;$i-lt6;$i++){$id=Unhex $before[$i].observation.file_id_info24;if($id.Length-ne24-or[BitConverter]::ToUInt64($id,0)-ne[UInt64]$x.expected_volume_serial){throw 'Actual volume identity mismatch'};$basic=Unhex $before[$i].observation.basic40;$attrs=[BitConverter]::ToUInt32($basic,32);if($attrs-ne$(if($i-eq5){16}else{32})){throw 'Unexpected scratch file/directory storage kind'}}
  foreach($i in @(0,1,5)){[AliasBasic4Cycle]::ValidateBasic((Unhex $before[$i].observation.basic40))}
  if($before[0].observation.file_id_info24-cne$before[2].observation.file_id_info24-or$before[1].observation.file_id_info24-cne$before[3].observation.file_id_info24-or$before[0].observation.file_id_info24-ceq$before[1].observation.file_id_info24){throw 'Hardlink/two-file identity precondition failed'}
  $rows=$report.directory_before;$ra=@($rows|Where-Object{$_.Name-ceq'LongAlphaPrimaryFile.bin'});$rb=@($rows|Where-Object{$_.Name-ceq'LongBetaPrimaryFile.bin'});if($ra.Count-ne1-or$rb.Count-ne1-or$ra[0].ShortName-cne'ALPHAA~1.BIN'-or$rb[0].ShortName-cne'ALPHAB~1.BIN'){throw 'Original two-alias ownership precondition failed'}
  $startingAliasA=Observation ('\\?\'+(Join-Path $root 'ALPHAA~1.BIN'));$startingAliasB=Observation ('\\?\'+(Join-Path $root 'ALPHAB~1.BIN'));$report.starting_alias_observations=@($startingAliasA,$startingAliasB);if(-not(Same $startingAliasA $before[0].observation)-or-not(Same $startingAliasB $before[1].observation)){throw 'Direct starting alias ownership/metadata mismatch before mutation'}
  $negative=[AliasBasic4Cycle]::Alias(('\\?\'+$a),'',$false);$afterNegative=Snapshot $paths;$report.negative_access_status=$negative;$report.after_negative_access=$afterNegative;if($negative-ne5-or-not(SnapshotSame $before $afterNegative)){throw 'Negative access control changed metadata/expectedstatus'}
  $collision=[AliasBasic4Cycle]::Alias(('\\?\'+$a),'ALPHAB~1.BIN',$true);$afterCollision=Snapshot $paths;$report.negative_collision_status=$collision;$report.after_negative_collision=$afterCollision;if($collision-ne183-or-not(SnapshotSame $before $afterCollision)){throw 'Known collision control changed metadata/expectedstatus'}
  foreach($p in @($a,$b)){$status=[AliasBasic4Cycle]::Alias(('\\?\'+$p),'',$true);$actions+=@{phase='clear-owned-alias';path=$p;status=$status};if($status-ne0){throw 'Owned alias clear failed'}}
  $report.after_clear=Snapshot $paths;$report.directory_after_clear=[AliasBasic4Cycle]::Enumerate($nativeRoot)
  foreach($pair in @(@($a,'ALPHAB~1.BIN'),@($b,'ALPHAA~1.BIN'))){$status=[AliasBasic4Cycle]::Alias(('\\?\'+$pair[0]),$pair[1],$true);$actions+=@{phase='assign-opposite';path=$pair[0];name=$pair[1];status=$status};if($status-ne0){throw 'Opposite alias assignment failed'}}
  $report.after_setter=Snapshot $paths
  foreach($i in @(0,1,5)){$status=[AliasBasic4Cycle]::Basic(('\\?\'+$paths[$i]),(Unhex $before[$i].observation.basic40));$actions+=@{phase='restore-own-basic4';path=$paths[$i];status=$status};if($status-ne0){throw 'Own four-time restore failed'}}
  $report.directory_after=[AliasBasic4Cycle]::Enumerate($nativeRoot)
  $aa=@($report.directory_after|Where-Object{$_.Name-ceq'LongAlphaPrimaryFile.bin'});$bb=@($report.directory_after|Where-Object{$_.Name-ceq'LongBetaPrimaryFile.bin'});$aliasA=Observation ('\\?\'+(Join-Path $root 'ALPHAB~1.BIN'));$aliasB=Observation ('\\?\'+(Join-Path $root 'ALPHAA~1.BIN'));$report.opposite_alias_observations=@($aliasA,$aliasB);$report.after_restore=Snapshot $paths
  $report.success=(SnapshotSame $before $report.after_restore)-and$aa.Count-eq1-and$bb.Count-eq1-and$aa[0].ShortName-ceq'ALPHAB~1.BIN'-and$bb[0].ShortName-ceq'ALPHAA~1.BIN'-and(Same $aliasA $before[0].observation)-and(Same $aliasB $before[1].observation)
  if(-not$report.success){throw 'Two-cycle/own metadata final gate failed'}
 }
}catch{$report.error=$_.Exception.Message}finally{$report.scratch_root=$root;$report.actions=$actions;$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 35));$writer.Flush()}finally{$stream.Dispose()}}
if(-not$report.success){throw 'Alias Basic4 control failed; original baseline and report preserved'}
