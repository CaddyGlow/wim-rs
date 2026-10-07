# NEW scratch objects only, on a NEW child of an independently initialized volume.
param([Parameter(Mandatory=$true)][string]$InputJson,[Parameter(Mandatory=$true)][string]$ExpectedInputSHA256,
 [Parameter(Mandatory=$true)][string]$ScratchVolumeRoot,[Parameter(Mandatory=$true)][string]$ExpectedDiskSerial,
 [Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
if((Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash.ToLowerInvariant()-cne$ExpectedInputSHA256.ToLowerInvariant()){throw 'Input SHA mismatch'}
$inputData=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($inputData.schema-ne1-or$inputData.mode-cne'objectid-directory-basic4-scratch'-or$inputData.helper_sha256-cne(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()){throw 'Mode/helper binding mismatch'}
$drive=Get-Item -LiteralPath $ScratchVolumeRoot;if($drive.FullName-notmatch'^[A-Za-z]:\\$'-or$drive.FullName.Substring(0,2)-ieq$env:SystemDrive){throw 'Dedicated scratch root required'}
$disk=Get-Partition -DriveLetter $drive.FullName.Substring(0,1)|Get-Disk;$volume=Get-Volume -DriveLetter $drive.FullName.Substring(0,1)
if($disk.IsBoot-or$disk.IsSystem-or$disk.SerialNumber.Trim()-cne$ExpectedDiskSerial-or$ExpectedDiskSerial-cne$inputData.disk_serial-or$volume.FileSystem-cne'NTFS'-or$volume.UniqueId.TrimEnd('\').Split('\')[-1]-notlike('*'+$inputData.volume_guid+'*')){throw 'Initialized scratch serial/volume identity mismatch'}
$service=Get-CimInstance Win32_Service -Filter "Name='TrkWks'";if($service.State-cne'Running'-or$service.StartMode-cne'Auto'){throw 'Normal tracking service context missing'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.ComponentModel;
using Microsoft.Win32.SafeHandles;
public static class ObjectIdDirectoryBasic4Scratch {
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
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] b,uint size,out uint needed);
 [DllImport("ntdll.dll")] static extern int NtSetInformationFile(SafeFileHandle h,out Status status,IntPtr buffer,uint length,int kind);
 static void Error(){throw new Win32Exception(Marshal.GetLastWin32Error());}
 public static void Enable(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string name in new[]{"SeBackupPrivilege","SeSecurityPrivilege"}){var p=new Priv{count=1,attributes=2};if(!LookupPrivilegeValue(null,name,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();int error=Marshal.GetLastWin32Error();if(error!=0)throw new Win32Exception(error);}}finally{CloseHandle(t);}}
 static SafeFileHandle Open(string p,bool write){var h=CreateFileW(p,0x01020080u|(write?0x100u:0),7,IntPtr.Zero,3u,0x02200000,IntPtr.Zero);if(h.IsInvalid)Error();return h;}
 static byte[] Object(SafeFileHandle h,bool setup){var b=new byte[64];uint n;if(!DeviceIoControl(h,setup?0x900c0u:0x9009cu,null,0,b,64,out n,IntPtr.Zero))Error();if(n!=64)throw new Exception("Full64 required");return b;}
 public static void ValidateDirectory(byte[] b){if(b.Length!=40||BitConverter.ToUInt32(b,32)!=0x11u)throw new ArgumentException("Exactly readonly directory required");}
 public static void Setup(string p){using(var h=Open(p,true)){var b=new byte[40];if(!GetFileInformationByHandleEx(h,0,b,40))Error();ValidateDirectory(b);Object(h,true);}}
 static byte[][] Capture(SafeFileHandle h,bool objectId=true){var basic=new byte[40];var id=new byte[24];if(!GetFileInformationByHandleEx(h,0,basic,40)||!GetFileInformationByHandleEx(h,18,id,24))Error();if((BitConverter.ToUInt32(basic,32)&0x400)!=0)throw new Exception("Reparse forbidden");var sd=new byte[1048576];uint n;int status=NtQuerySecurityObject(h,0x1ff,sd,(uint)sd.Length,out n);if(status<0||n<20||n>sd.Length)throw new Exception("Security query "+status);Array.Resize(ref sd,(int)n);return new[]{basic,id,sd,objectId?Object(h,false):new byte[0]};}
 public static byte[][] Observe(string p,bool objectId=true){using(var h=Open(p,false)){return Capture(h,objectId);}}
 public static void ValidateBasic(byte[] b){if(b.Length!=40)throw new ArgumentException("FILE_BASIC_INFORMATION length");for(int o=0;o<32;o+=8){long v=BitConverter.ToInt64(b,o);if(v<=0)throw new ArgumentException("Literal zero/sentinel time cannot be restored exactly");}if(BitConverter.ToUInt32(b,32)==0)throw new ArgumentException("Zero attributes preserve current, not exact restore");}
 static int Restore(SafeFileHandle h,byte[] b){ValidateBasic(b);var copy=(byte[])b.Clone();Array.Clear(copy,36,4);var pinned=GCHandle.Alloc(copy,GCHandleType.Pinned);try{Status status;int result=NtSetInformationFile(h,out status,pinned.AddrOfPinnedObject(),40,4);return result;}finally{pinned.Free();}}
 public sealed class Result{public byte[][] Before,AfterExtended,AfterRestore,AfterNegative;public int NegativeStatus,BasicStatus;public uint ExtendedStatus;}
 public static Result RoundTrip(string p,byte[] extension,bool setExtension){if(extension.Length!=48)throw new ArgumentException("Full48 required");var r=new Result();using(var h=Open(p,true)){r.Before=Capture(h);ValidateBasic(r.Before[0]);using(var negative=Open(p,false)){r.NegativeStatus=Restore(negative,r.Before[0]);}r.AfterNegative=Capture(h);if(setExtension){uint n;if(!DeviceIoControl(h,0x900bc,extension,48,null,0,out n,IntPtr.Zero))r.ExtendedStatus=(uint)Marshal.GetLastWin32Error();}r.AfterExtended=Capture(h);r.BasicStatus=Restore(h,r.Before[0]);r.AfterRestore=Capture(h);}return r;}
}
'@
function Hex([byte[]]$b){[BitConverter]::ToString($b).Replace('-','').ToLowerInvariant()}
function Unhex([string]$s){if($s-cnotmatch'^(?:[0-9a-f]{2}){48}$'){throw 'Source48 bounds'};[byte[]]$b=for($i=0;$i-lt$s.Length;$i+=2){[Convert]::ToByte($s.Substring($i,2),16)};return ,$b}
function Observation([byte[][]]$b){return @{basic36=(Hex ([byte[]]$b[0][0..35]));basic40=(Hex $b[0]);basic4=@{creation_time=[BitConverter]::ToUInt64($b[0],0);access_time=[BitConverter]::ToUInt64($b[0],8);write_time=[BitConverter]::ToUInt64($b[0],16);change_time=[BitConverter]::ToUInt64($b[0],24);attributes=[BitConverter]::ToUInt32($b[0],32)};file_id_info24=(Hex $b[1]);security_raw=(Hex $b[2]);object_id_full64=(Hex $b[3])}}
$privilegesBefore=(& whoami.exe /all|Out-String);[ObjectIdDirectoryBasic4Scratch]::Enable();$privilegesAfter=(& whoami.exe /all|Out-String)
$root=Join-Path $drive.FullName ('objectid-directory-basic4-scratch-'+[Guid]::NewGuid().ToString('N'));if(Test-Path -LiteralPath $root){throw 'Scratch collision'};New-Item -ItemType Directory -Path $root|Out-Null
$createdIds=[System.Collections.Generic.HashSet[string]]::new();$extension=Unhex $inputData.source_extended48;if(-not(@($extension|Where-Object {$_-ne0}).Count)){throw 'Nonzero source48 required'};$results=@()
foreach($setExtension in @($false,$true)){
 $label=if($setExtension){'readonly-directory-extended48-and-basic4-restore'}else{'readonly-directory-positive-basic4-roundtrip'}
 $path=Join-Path $root $label;$sibling=Join-Path $root ($label+'-sibling');$child=Join-Path $path 'untouched-child.bin'
 [IO.Directory]::CreateDirectory($path)|Out-Null;[IO.Directory]::CreateDirectory($sibling)|Out-Null;[IO.File]::WriteAllText($child,'untouched child')
 [IO.File]::SetAttributes($path,[IO.FileAttributes]::ReadOnly);[ObjectIdDirectoryBasic4Scratch]::Setup($path)
 $siblingBefore=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($sibling,$false));$childBefore=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($child,$false));$parentBefore=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($root,$false));$raw=[ObjectIdDirectoryBasic4Scratch]::RoundTrip($path,$extension,$setExtension);$before=Observation $raw.Before
 if($before.basic4.attributes-ne17-or$before.object_id_full64.Substring(0,32)-ceq('0'*32)-or-not$createdIds.Add($before.object_id_full64.Substring(0,32))-or[BitConverter]::ToUInt64($raw.Before[1],0)-ne[UInt64]$inputData.volume_serial){throw 'New readonly directory identity/initialized serial mismatch'}
 $afterNegative=Observation $raw.AfterNegative;$afterExtended=Observation $raw.AfterExtended;$afterRestore=Observation $raw.AfterRestore;$afterReopen=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($path));$siblingAfter=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($sibling,$false));$childAfter=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($child,$false));$parentAfter=Observation ([ObjectIdDirectoryBasic4Scratch]::Observe($root,$false))
 $expected64=if($setExtension){$before.object_id_full64.Substring(0,32)+$inputData.source_extended48}else{$before.object_id_full64}
 $negativeUnchanged=$before.basic36-ceq$afterNegative.basic36-and$before.security_raw-ceq$afterNegative.security_raw-and$before.file_id_info24-ceq$afterNegative.file_id_info24-and$before.object_id_full64-ceq$afterNegative.object_id_full64
 $restored=$before.basic36-ceq$afterRestore.basic36-and$before.basic36-ceq$afterReopen.basic36
 $preserved=$before.security_raw-ceq$afterRestore.security_raw-and$before.security_raw-ceq$afterReopen.security_raw-and$before.file_id_info24-ceq$afterRestore.file_id_info24-and$afterRestore.object_id_full64-ceq$expected64-and$before.file_id_info24-ceq$afterReopen.file_id_info24-and$afterReopen.object_id_full64-ceq$expected64
 $siblingPreserved=$siblingBefore.basic36-ceq$siblingAfter.basic36-and$siblingBefore.security_raw-ceq$siblingAfter.security_raw-and$siblingBefore.file_id_info24-ceq$siblingAfter.file_id_info24
 $childPreserved=$childBefore.basic36-ceq$childAfter.basic36-and$childBefore.security_raw-ceq$childAfter.security_raw-and$childBefore.file_id_info24-ceq$childAfter.file_id_info24
 $parentPreserved=$parentBefore.basic36-ceq$parentAfter.basic36-and$parentBefore.security_raw-ceq$parentAfter.security_raw-and$parentBefore.file_id_info24-ceq$parentAfter.file_id_info24
 $results+=@{parent_before=$parentBefore;parent_after=$parentAfter;parent_preserved=$parentPreserved;case=$label;path=$path;path_utf16_le=(Hex ([Text.Encoding]::Unicode.GetBytes($path)));child=$child;child_utf16_le=(Hex ([Text.Encoding]::Unicode.GetBytes($child)));child_before=$childBefore;child_after=$childAfter;child_preserved=$childPreserved;sibling=$sibling;setup_create_or_get_new_objects_only=$true;before=$before;negative_ntstatus=$raw.NegativeStatus;after_negative=$afterNegative;negative_unchanged=$negativeUnchanged;extended_set_status=$raw.ExtendedStatus;after_extended_set=$afterExtended;basic4_restore_ntstatus=$raw.BasicStatus;after_restore_same_handle=$afterRestore;after_reopen=$afterReopen;sibling_before=$siblingBefore;sibling_after=$siblingAfter;expected_full64=$expected64;exact_four_times_and_attributes_restored=$restored;full64_identity_security_preserved=$preserved;sibling_preserved=$siblingPreserved;passed=($raw.NegativeStatus-eq-1073741790-and$negativeUnchanged-and$raw.ExtendedStatus-eq0-and$raw.BasicStatus-eq0-and$restored-and$preserved-and$siblingPreserved-and$childPreserved-and$parentPreserved)}
}
$report=@{schema=1;mode='objectid-directory-basic4-scratch';executed=$true;production_executable=$false;input_sha256=$ExpectedInputSHA256;helper_sha256=$inputData.helper_sha256;initialized_parent_sha256=$inputData.initialized_parent_sha256;initialized_volume_object_id_bound_context=$inputData.volume_object_id;tracking_log_sha256_bound_context=$inputData.tracking_log_sha256;source_inventory_sha256=$inputData.source_inventory_sha256;scratch_root=$root;disk_serial=$disk.SerialNumber;volume_unique_id=$volume.UniqueId;kernel=[Environment]::OSVersion.Version.ToString();boot_utc=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');trk_wks_before=($service|Select-Object State,StartMode,ProcessId);trk_wks_after=(Get-CimInstance Win32_Service -Filter "Name='TrkWks'"|Select-Object State,StartMode,ProcessId);privileges_before=$privilegesBefore;privileges_after=$privilegesAfter;results=$results;online_passed=(@($results|Where-Object {-not$_.passed}).Count-eq0);offline_verified=$false;normal_reboot_verified=$false;scope='NEW readonly directories with runtime attributes 0x11 only; no directory hardlinks. NEW own16 scratch objects only; source48 setter plus exact positive four-time/attribute FileBasicInformation4 restore. No installed setter, volume identity copy, recurring hook or service changes.'}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 30));$writer.Flush()}finally{$stream.Dispose()}
if(-not$report.online_passed){throw 'Basic4 scratch gate failed; evidence preserved'}
