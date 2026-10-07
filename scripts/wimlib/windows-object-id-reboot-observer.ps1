# GET-only existing scratch observations; no creation, ObjectID setter or repair.
param([Parameter(Mandatory=$true)][string]$InputJson,
      [Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $ReportPath){throw 'Existing report forbidden'}
$inputData=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
$expectedScratchRoot='T:\objectid-scratch-8e13fcf95dd746739b3ba4f1b14a64af'
if($inputData.scratch_root -cne $expectedScratchRoot){throw 'Unexpected frozen scratch root'}
if($inputData.schema -ne 1 -or $inputData.mode -ne 'GET-only-existing-scratch' -or $inputData.rows.Count -lt 1){throw 'Invalid observer input'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class ObjectIdReadOnlyObserver {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr sa,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle handle,uint code,IntPtr input,uint inputLength,byte[] output,uint outputLength,out uint returned,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,byte[] output,uint size);
 public static byte[][] Observe(string path) {
  using(var handle=CreateFileW(path,0x80000000,7,IntPtr.Zero,3,0x00200000,IntPtr.Zero)) {
   if(handle.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var objectId=new byte[64];uint returned;
   if(!DeviceIoControl(handle,0x9009c,IntPtr.Zero,0,objectId,64,out returned,IntPtr.Zero))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   if(returned!=64)throw new InvalidOperationException("Expected full64 ObjectID");
   var identity=new byte[24];if(!GetFileInformationByHandleEx(handle,18,identity,24))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   return new[]{objectId,identity};
  }
 }
}
'@
function Hex([byte[]]$value){return [BitConverter]::ToString($value).Replace('-','').ToLowerInvariant()}
function Decode-Selector([string]$raw){if($raw.Length % 4){throw 'Invalid UTF16 length'};[char[]]$units=for($i=0;$i -lt $raw.Length;$i+=4){$lo=[Convert]::ToByte($raw.Substring($i,2),16);$hi=[Convert]::ToByte($raw.Substring($i+2,2),16);[char]($lo+256*$hi)};return [string]::new($units)}
$rows=@()
foreach($expected in $inputData.rows){
 $row=@{path_utf16_le=$expected.path_utf16_le;object_id_full64=$null;file_id_info24=$null;passed=$false;error=$null}
 try{
  $path=Decode-Selector $expected.path_utf16_le
  if($path.Contains([string][char]0) -or $path.Contains('/') -or -not $path.StartsWith($expectedScratchRoot+'\',[StringComparison]::Ordinal) -or @($path.Substring(3).Split([char]92)|Where-Object {$_ -eq '' -or $_ -eq '.' -or $_ -eq '..'}).Count -ne 0){throw 'Selector outside exact existing scratch root'}
  $value=[ObjectIdReadOnlyObserver]::Observe($path)
  $row.object_id_full64=Hex $value[0];$row.file_id_info24=Hex $value[1]
  $row.passed=($row.object_id_full64 -ceq $expected.object_id_full64.ToLowerInvariant() -and $row.file_id_info24 -ceq $expected.file_id_info24.ToLowerInvariant())
 }catch{$row.error=$_.Exception.Message}
 $rows+=$row
}
$report=@{schema=1;mode='GET-only-existing-scratch';input_sha256=(Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash;kernel=[Environment]::OSVersion.Version.ToString();last_boot_utc=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o');observed_utc=[DateTime]::UtcNow.ToString('o');rows=$rows;all_passed=(@($rows|Where-Object {-not $_.passed}).Count -eq 0)}
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write(($report|ConvertTo-Json -Depth 20));$writer.Flush()}finally{$stream.Dispose()}
if(-not $report.all_passed){throw 'GET-only persistence gate failed; evidence preserved'}
