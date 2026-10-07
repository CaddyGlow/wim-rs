# Preparation only: execute after immutable baseline audit on a disposable branch.
# A dedicated new NTFS scratch volume is required. Source paths are never opened.
param([Parameter(Mandatory=$true)][string]$InputJson,
      [Parameter(Mandatory=$true)][string]$ScratchVolumeRoot,
      [Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
$drive=Get-Item -LiteralPath $ScratchVolumeRoot
if($drive.FullName -notmatch '^[A-Za-z]:\\$' -or $drive.FullName.Substring(0,2) -eq $env:SystemDrive){throw 'Dedicated scratch volume required'}
if((Get-Volume -DriveLetter $drive.FullName.Substring(0,1)).FileSystem -ne 'NTFS'){throw 'NTFS required'}
$inputData=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
if($inputData.schema -ne 1 -or $inputData.challenges.Count -lt 1){throw 'Invalid input'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class ObjectIdScratch {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr sa,uint d,uint f,IntPtr t);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint code,byte[] input,uint n,byte[] output,uint size,out uint returned,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool FlushFileBuffers(SafeFileHandle h);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateHardLinkW(string alias,string original,IntPtr security);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetShortPathNameW(string path,StringBuilder shortPath,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle h,int kind,byte[] info,uint size);
 public static SafeFileHandle Open(string p,bool create) {
  // CREATE_NEW only for unique scratch setup; observations open existing no-follow.
  var h=CreateFileW(p,create?0xC0000000u:0x80000000u,7,IntPtr.Zero,create?1u:3u,0x00200000,IntPtr.Zero);
  if(h.IsInvalid) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return h;
 }
 public static byte[] Query(SafeFileHandle h,bool setup) {
  var b=new byte[64];uint n;
  // CREATE_OR_GET is used once, exclusively on the new scratch file.
  if(!DeviceIoControl(h,setup?0x900c0u:0x9009cu,null,0,b,64,out n,IntPtr.Zero)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  if(n!=64)throw new InvalidOperationException("ObjectID result must be full64");return b;
 }
 public static uint SetExtended(SafeFileHandle h,byte[] extension) {
  if(extension.Length!=48)throw new ArgumentException("extended48 required");uint n;
  if(!DeviceIoControl(h,0x900bc,extension,48,null,0,out n,IntPtr.Zero))return (uint)Marshal.GetLastWin32Error();
  if(!FlushFileBuffers(h))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return 0;
 }
 public static byte[] Identity(SafeFileHandle h) {var b=new byte[24];if(!GetFileInformationByHandleEx(h,18,b,24))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return b;}
 public static void Link(string alias,string original){if(!CreateHardLinkW(alias,original,IntPtr.Zero))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}
 public static string Short(string p){var b=new StringBuilder(32768);uint n=GetShortPathNameW(p,b,32768);if(n==0)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());if(n>=32768)throw new InvalidOperationException("short path truncated");return b.ToString();}
}
'@
function Hex([byte[]]$b){return [BitConverter]::ToString($b).Replace('-','').ToLowerInvariant()}
function Unhex([string]$s){if($s.Length % 2){throw 'Invalid hex'};[byte[]]$b=for($j=0;$j -lt $s.Length;$j+=2){[Convert]::ToByte($s.Substring($j,2),16)};return ,$b}
$root=Join-Path $drive.FullName ('objectid-scratch-'+[Guid]::NewGuid().ToString('N'))
if(Test-Path -LiteralPath $root){throw 'Scratch collision'}
New-Item -ItemType Directory -Path $root|Out-Null
$results=@();$i=0;$createdIds=[System.Collections.Generic.HashSet[string]]::new()
foreach($challenge in $inputData.challenges){
 $extension=Unhex $challenge.extended48_hex
 if($extension.Length -ne 48 -or -not (@($extension[0..31]|Where-Object {$_ -ne 0}).Count)){throw 'Nonzero birth32 challenge required'}
 $path=Join-Path $root ('scratch long object identifier challenge '+$i+'.bin')
 $alias=Join-Path $root ('alias-'+$i+'.bin')
 $h=[ObjectIdScratch]::Open($path,$true)
 try{$before=[ObjectIdScratch]::Query($h,$true);if(-not (@($before[0..15]|Where-Object {$_ -ne 0}).Count) -or -not $createdIds.Add((Hex $before[0..15]))){throw 'Invalid or duplicate scratch-generated ObjectID'};$identity=[ObjectIdScratch]::Identity($h);$setStatus=[ObjectIdScratch]::SetExtended($h,$extension)}finally{$h.Dispose()}
 [ObjectIdScratch]::Link($alias,$path)
 $short=[ObjectIdScratch]::Short($path)
 $observations=@()
 foreach($observe in @($path,$alias,$short)){
  $h=[ObjectIdScratch]::Open($observe,$false)
  try{$after=[ObjectIdScratch]::Query($h,$false);$id=[ObjectIdScratch]::Identity($h)}finally{$h.Dispose()}
  $observations+=@{path=$observe;path_utf16_le=(Hex ([Text.Encoding]::Unicode.GetBytes($observe)));object_id_full64=(Hex $after);file_id_info24=(Hex $id);first16_preserved=((Hex $after[0..15]) -eq (Hex $before[0..15]));extended48_exact=((Hex $after[16..63]) -eq (Hex $extension));alias_identity_exact=((Hex $id) -eq (Hex $identity))}
 }
 $passed=($setStatus -eq 0 -and @($observations|Where-Object {-not $_.first16_preserved -or -not $_.extended48_exact -or -not $_.alias_identity_exact}).Count -eq 0)
 $results+=@{case=$i;setup_create_or_get_only=$true;before_full64=(Hex $before);expected_extended48=(Hex $extension);set_status=$setStatus;observations_get_only=$observations;distinct_dos_alias=($short -ne $path);passed=$passed};$i++
}
$report=@{schema=1;executed=$true;scratch_root=$root;fixture_sha256=$inputData.fixture_sha256;input_sha256=(Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash;kernel=[Environment]::OSVersion.Version.ToString();identity=(& whoami.exe /all|Out-String);results=$results;online_passed=(@($results|Where-Object {-not $_.passed}).Count -eq 0);independent_offline_verified=$false}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 20));$writer.Flush()}finally{$stream.Dispose()}
if(-not $report.online_passed){throw 'Scratch online gate failed; evidence preserved'}
