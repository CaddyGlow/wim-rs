# GET-only metadata observer. Input is externally bound to immutable source/target receipts.
param([Parameter(Mandatory=$true)][string]$InputJson,
 [Parameter(Mandatory=$true)][string]$ExpectedInputSHA256,
 [Parameter(Mandatory=$true)][string]$SourceInventory,
 [Parameter(Mandatory=$true)][string]$TargetContextReceipt,
 [Parameter(Mandatory=$true)][string]$TargetRawContext,
 [Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
if((Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedInputSHA256.ToLowerInvariant()){throw 'Input hash mismatch'}
$helperSHA=(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
$inputData=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($helperSHA -cne $inputData.observer_helper_sha256){throw 'Observer helper SHA mismatch'}
if($inputData.schema -ne 1 -or $inputData.mode -cne 'GET-only-installed-objectids' -or $inputData.windows_system_drive -cne 'C:'){throw 'Unsupported observer input'}
if((Get-FileHash -LiteralPath $SourceInventory -Algorithm SHA256).Hash.ToLowerInvariant() -cne $inputData.source_inventory_sha256){throw 'Source inventory hash mismatch'}
foreach($hash in @($inputData.source_inventory_sha256,$inputData.capture_wim_sha256,$inputData.target_snapshot_sha256,$inputData.target_context_receipt_sha256)){if($hash -cnotmatch '^[0-9a-f]{64}$'){throw 'Invalid provenance digest'}}
if($inputData.target_volume_object_id -cnotmatch '^[0-9a-f]{32}$'){throw 'Invalid target volume context'}
if((Get-FileHash -LiteralPath $TargetContextReceipt -Algorithm SHA256).Hash.ToLowerInvariant() -cne $inputData.target_context_receipt_sha256){throw 'Target context receipt hash mismatch'}
$receipt=Get-Content -LiteralPath $TargetContextReceipt -Raw|ConvertFrom-Json
if($receipt.image_sha256 -cne $inputData.target_snapshot_sha256 -or -not $receipt.authoritative_vm_terminal -or -not $receipt.strict_clean_ntfs_open -or -not $receipt.verified_physical_sha256_against_frozen_handoff){throw 'Target receipt lacks frozen clean handoff'}
if((Get-FileHash -LiteralPath $TargetRawContext -Algorithm SHA256).Hash.ToLowerInvariant() -cne $receipt.context_sha256){throw 'Raw target context hash mismatch'}
$context=Get-Content -LiteralPath $TargetRawContext -Raw|ConvertFrom-Json
$volumeObjects=@($context.records|Where-Object {$_.record -eq 3}|ForEach-Object {$_.attributes}|Where-Object {$_.type -ceq 'ObjectId'})
if($context.image -cne $receipt.image -or [string]$context.partition -cne [string]$receipt.partition -or $volumeObjects.Count -ne 1 -or $volumeObjects[0].raw_hex -cne $inputData.target_volume_object_id){throw 'Raw volume context binding mismatch'}
$inventory=Get-Content -LiteralPath $SourceInventory -Raw|ConvertFrom-Json
$sourceRows=@();foreach($group in $inventory.groups){foreach($path in $group.paths){$sourceRows+=@{path_utf16_le=$path.path_utf16_le;source_full64=$group.object_id_full64;source_file_reference=$group.file_reference}}}
if($sourceRows.Count -ne $inputData.rows.Count -or $sourceRows.Count -ne $inventory.object_id_paths){throw 'Selector scope incomplete'}
for($i=0;$i -lt $sourceRows.Count;$i++){foreach($key in @('path_utf16_le','source_full64','source_file_reference')){if([string]$sourceRows[$i][$key] -cne [string]$inputData.rows[$i].$key){throw 'Source selector mismatch'}}}
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class InstalledObjectIdObserver {
 [StructLayout(LayoutKind.Sequential)] struct Luid {public uint low; public int high;}
 [StructLayout(LayoutKind.Sequential)] struct Priv {public uint count;public Luid luid;public uint attributes;}
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint access,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid luid);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr token,bool disable,ref Priv p,uint length,IntPtr previous,IntPtr needed);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr sa,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle handle,uint code,IntPtr input,uint inputLength,byte[] output,uint outputLength,out uint returned,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,byte[] output,uint size);
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] sd,uint length,out uint needed);
 static void Error(){throw new Win32Exception(Marshal.GetLastWin32Error());}
 public static void EnableReadPrivileges(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string name in new[]{"SeBackupPrivilege","SeSecurityPrivilege"}){Priv p=new Priv();p.count=1;p.attributes=2;if(!LookupPrivilegeValue(null,name,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();if(Marshal.GetLastWin32Error()==1300)throw new Exception(name+" not assigned");}}finally{CloseHandle(t);}}
 public static byte[][] Observe(string path){
  // READ_CONTROL | ACCESS_SYSTEM_SECURITY | FILE_READ_ATTRIBUTES; OPEN_EXISTING.
  using(var h=CreateFileW(path,0x01020080,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){
   if(h.IsInvalid)Error();var basicBefore=new byte[40];if(!GetFileInformationByHandleEx(h,0,basicBefore,40))Error();
   if((BitConverter.ToUInt32(basicBefore,32)&0x400)!=0)throw new Exception("Reparse endpoint forbidden");
   var id=new byte[24];if(!GetFileInformationByHandleEx(h,18,id,24))Error();
   var objectId=new byte[64];uint n;if(!DeviceIoControl(h,0x9009c,IntPtr.Zero,0,objectId,64,out n,IntPtr.Zero))Error();if(n!=64)throw new Exception("Expected full64 ObjectID");
   var sd=new byte[1048576];int status=NtQuerySecurityObject(h,0x1ff,sd,(uint)sd.Length,out n);if(status<0||n<20||n>sd.Length)throw new Exception("NtQuerySecurityObject status "+status+" length "+n);Array.Resize(ref sd,(int)n);
   var basicAfter=new byte[40];if(!GetFileInformationByHandleEx(h,0,basicAfter,40))Error();
   return new[]{id,objectId,sd,basicBefore,basicAfter};
  }
 }
}
'@
function Hex([byte[]]$b){[BitConverter]::ToString($b).Replace('-','').ToLowerInvariant()}
function LogicalBasic([byte[]]$b){if($b.Length -ne 40){throw 'Invalid FILE_BASIC_INFO length'};return Hex ([byte[]]$b[0..35])}
function Metadata([byte[]]$b,[byte[]]$sd){@{creation_time=[BitConverter]::ToUInt64($b,0);access_time=[BitConverter]::ToUInt64($b,8);write_time=[BitConverter]::ToUInt64($b,16);change_time=[BitConverter]::ToUInt64($b,24);attributes=[BitConverter]::ToUInt32($b,32);security_raw=(Hex $sd)}}
function Decode([string]$raw){if($raw -cnotmatch '^(?:[0-9a-f]{4})+$'){throw 'Invalid UTF16 selector'};[char[]]$u=for($i=0;$i -lt $raw.Length;$i+=4){[char]([Convert]::ToByte($raw.Substring($i,2),16)+256*[Convert]::ToByte($raw.Substring($i+2,2),16))};$p=[string]::new($u);if(-not $p.StartsWith('/') -or $p.Contains('\') -or $p.Contains(':') -or $p.Contains([string][char]0) -or @($p.Substring(1).Split('/')|Where-Object {$_ -eq '' -or $_ -eq '.' -or $_ -eq '..'}).Count){throw 'Path escape forbidden'};return '\\?\C:'+ $p.Replace('/','\')}
[InstalledObjectIdObserver]::EnableReadPrivileges()
$rows=@()
foreach($expected in $inputData.rows){
 $row=@{path_utf16_le=$expected.path_utf16_le;source_file_reference=$expected.source_file_reference;source_full64=$expected.source_full64;identity_observation='WindowsFileIdInfo';error=$null;passed=$false}
 try{
  $path=Decode $expected.path_utf16_le;$before=[InstalledObjectIdObserver]::Observe($path);$after=[InstalledObjectIdObserver]::Observe($path)
  $row.first_file_id_info24=Hex $before[0];$row.first_object_id_full64=Hex $before[1];$row.file_id_info24=Hex $after[0];$row.object_id_full64=Hex $after[1];$row.metadata=Metadata $after[3] $after[2]
  $row.first_metadata=Metadata $before[3] $before[2];$row.first_after_get_metadata=Metadata $before[4] $before[2];$row.reopened_after_get_metadata=Metadata $after[4] $after[2]
  $row.identity_stable=((Hex $before[0]) -ceq $row.file_id_info24);$row.full64_stable=((Hex $before[1]) -ceq $row.object_id_full64)
  $row.raw_basic_info40=@{first_before=(Hex $before[3]);first_after=(Hex $before[4]);reopened_before=(Hex $after[3]);reopened_after=(Hex $after[4])}
  $row.security_stable=((Hex $before[2]) -ceq (Hex $after[2]));$row.basic4_and_attributes_stable=((LogicalBasic $before[3]) -ceq (LogicalBasic $before[4]) -and (LogicalBasic $before[3]) -ceq (LogicalBasic $after[3]) -and (LogicalBasic $after[3]) -ceq (LogicalBasic $after[4]))
  $row.target_volume_matches=([BitConverter]::ToUInt64($after[0],0) -eq [UInt64]$inputData.target_volume_serial)
  $row.existing_objectid16_matches_source=($row.object_id_full64.Substring(0,32) -ceq $expected.source_full64.Substring(0,32))
  $row.full64_matches_source=($row.object_id_full64 -ceq $expected.source_full64)
  $row.passed=$row.identity_stable -and $row.full64_stable -and $row.security_stable -and $row.basic4_and_attributes_stable -and $row.target_volume_matches -and $row.existing_objectid16_matches_source
 }catch{$row.error=$_.Exception.Message};$rows+=$row
}
$report=@{schema=1;mode='GET-only-installed-objectids';input_sha256=$ExpectedInputSHA256.ToLowerInvariant();observer_helper_sha256=$helperSHA;source_inventory_sha256=$inputData.source_inventory_sha256;capture_wim_sha256=$inputData.capture_wim_sha256;target_snapshot_sha256=$inputData.target_snapshot_sha256;target_volume_object_id_bound_context=$inputData.target_volume_object_id;target_context_receipt_sha256=$inputData.target_context_receipt_sha256;privileges_enabled=@('SeBackupPrivilege','SeSecurityPrivilege');kernel=[Environment]::OSVersion.Version.ToString();last_boot_utc=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');trk_wks=(Get-CimInstance Win32_Service -Filter "Name='TrkWks'"|Select-Object State,StartMode,ProcessId);observed_utc=[DateTime]::UtcNow.ToString('o');rows=$rows;all_observations_stable=(@($rows|Where-Object {-not $_.passed}).Count -eq 0);full_source64_fidelity=(@($rows|Where-Object {-not $_.full64_matches_source}).Count -eq 0);target_volume_object_id_observed_by_this_helper=$false;production_executable=$false}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 20));$writer.Flush()}finally{$stream.Dispose()}
if(-not $report.all_observations_stable){throw 'GET-only observation failed; evidence preserved'}
