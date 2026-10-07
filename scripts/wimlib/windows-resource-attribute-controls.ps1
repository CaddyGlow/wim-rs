# Run only after successful OOBE on a disposable branch and a NEW scratch NTFS volume.
# Source paths are provenance strings, never opened. No ACE20 setters exist here.
param([Parameter(Mandatory=$true)][string]$InputJson,
      [Parameter(Mandatory=$true)][string]$ScratchVolumeRoot,
      [Parameter(Mandatory=$true)][string]$ReportPath,
      [ValidateSet('Directory','File')][string]$ScratchObjectType='Directory',
      [switch]$MaximumAllowedControl)
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $ReportPath) { throw 'Report already exists' }
$drive = Get-Item -LiteralPath $ScratchVolumeRoot
if ($drive.FullName -notmatch '^[A-Za-z]:\\$') { throw 'Supply a dedicated scratch volume root' }
if ($drive.FullName.Substring(0,2) -eq $env:SystemDrive) { throw 'System volume forbidden' }
$vol = Get-Volume -DriveLetter $drive.FullName.Substring(0,1)
if ($vol.FileSystem -ne 'NTFS') { throw 'Scratch must be NTFS' }
$inputData = Get-Content -LiteralPath $InputJson -Raw | ConvertFrom-Json
if ($inputData.schema -ne 1 -or $inputData.aces.Count -ne 2) { throw 'Invalid input' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class AttributeScratch {
 public static bool MaximumAllowedControl=false;
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr sa,uint d,uint f,IntPtr t);
 [DllImport("advapi32.dll")] static extern uint GetSecurityInfo(SafeFileHandle h,uint type,uint info,out IntPtr owner,out IntPtr group,out IntPtr dacl,out IntPtr sacl,out IntPtr sd);
 [DllImport("advapi32.dll")] static extern uint SetSecurityInfo(SafeFileHandle h,uint type,uint info,IntPtr owner,IntPtr group,IntPtr dacl,IntPtr sacl);
 [DllImport("advapi32.dll")] static extern uint GetSecurityDescriptorLength(IntPtr sd);
 [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
 [DllImport("advapi32.dll", SetLastError=true)] static extern bool InitializeAcl(IntPtr acl,uint size,uint revision);
 [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool ConvertStringSidToSidW(string s,out IntPtr sid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool AddResourceAttributeAce(IntPtr acl,uint revision,uint flags,uint mask,IntPtr sid,IntPtr claims,out uint size);
 [StructLayout(LayoutKind.Sequential)] struct Claim { public IntPtr Name; public ushort Type,Reserved; public uint Flags,Count; public IntPtr Values; }
 [StructLayout(LayoutKind.Sequential)] struct Claims { public ushort Version,Reserved; public uint Count; public IntPtr Attributes; }
 [StructLayout(LayoutKind.Sequential)] struct Luid { public uint Low; public int High; }
 [StructLayout(LayoutKind.Sequential)] struct Privilege { public uint Count; public Luid Id; public uint Attributes; }
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint access,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValueW(string system,string name,out Luid id);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr token,bool disable,ref Privilege state,uint size,IntPtr old,IntPtr length);
 public static void EnableSecurityPrivilege() {
  IntPtr token; if(!OpenProcessToken(GetCurrentProcess(),0x28,out token)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  try { Luid id; if(!LookupPrivilegeValueW(null,"SeSecurityPrivilege",out id)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var state=new Privilege{Count=1,Id=id,Attributes=2};
   if(!AdjustTokenPrivileges(token,false,ref state,0,IntPtr.Zero,IntPtr.Zero)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   int e=Marshal.GetLastWin32Error(); if(e!=0) throw new System.ComponentModel.Win32Exception(e);
  } finally {CloseHandle(token);}
 }
 public static SafeFileHandle Open(string p,bool write) {
  var h=CreateFileW(p,write&&MaximumAllowedControl?0x03000000u:(0x1020000u|(write?0x40000u:0)),7,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
  if(h.IsInvalid) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); return h;
 }
 // Full raw owner/group/DACL/SACL/label/resource-attribute descriptor; query only.
 public static byte[] Query(SafeFileHandle h) {
  IntPtr o,g,d,s,sd; uint e=GetSecurityInfo(h,1,0x3f,out o,out g,out d,out s,out sd);
  if(e!=0) throw new System.ComponentModel.Win32Exception((int)e);
  try { byte[] b=new byte[GetSecurityDescriptorLength(sd)]; Marshal.Copy(sd,b,0,b.Length); return b; } finally {LocalFree(sd);}
 }
 public static uint Apply(SafeFileHandle h, byte[] ace) {
  if(ace.Length<28 || ace[0]!=18 || BitConverter.ToUInt16(ace,2)!=ace.Length) throw new ArgumentException("ACE18 envelope");
  var acl=Marshal.AllocHGlobal(8+ace.Length);
  try { if(!InitializeAcl(acl,(uint)(8+ace.Length),2)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   Marshal.WriteInt16(acl,4,1); Marshal.Copy(ace,0,IntPtr.Add(acl,8),ace.Length);
   return SetSecurityInfo(h,1,0x20,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,acl);
  } finally {Marshal.FreeHGlobal(acl);}
 }
 public static byte[] Construct() {
  IntPtr sid=IntPtr.Zero,name=Marshal.StringToHGlobalUni("CaptureScratch"),value=Marshal.AllocHGlobal(8),a=Marshal.AllocHGlobal(Marshal.SizeOf<Claim>()),c=Marshal.AllocHGlobal(Marshal.SizeOf<Claims>()),acl=Marshal.AllocHGlobal(4096);
  try {
   if(!ConvertStringSidToSidW("S-1-1-0",out sid)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   Marshal.WriteInt64(value,0x1122334455667788);
   Marshal.StructureToPtr(new Claim{Name=name,Type=2,Count=1,Values=value},a,false);
   Marshal.StructureToPtr(new Claims{Version=1,Count=1,Attributes=a},c,false);
   if(!InitializeAcl(acl,4096,2)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   uint size; if(!AddResourceAttributeAce(acl,2,0,0,sid,c,out size)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   int n=(ushort)Marshal.ReadInt16(acl,10); byte[] b=new byte[n]; Marshal.Copy(IntPtr.Add(acl,8),b,0,n); return b;
  } finally {if(sid!=IntPtr.Zero)LocalFree(sid); foreach(var p in new[]{name,value,a,c,acl})Marshal.FreeHGlobal(p);}
 }
}
'@
function Hex([byte[]]$bytes) { return [BitConverter]::ToString($bytes).Replace('-','').ToLowerInvariant() }
function Parse-SD([byte[]]$bytes) {
    function Slice([int]$offset,[int]$size) { if ($offset -eq 0) { return '' }; return Hex $bytes[$offset..($offset+$size-1)] }
    $o=[BitConverter]::ToInt32($bytes,4); $g=[BitConverter]::ToInt32($bytes,8)
    $s=[BitConverter]::ToInt32($bytes,12); $d=[BitConverter]::ToInt32($bytes,16)
    $aces=@();$otherAces=@(); if($s -ne 0) { $cursor=$s+8; $count=[BitConverter]::ToUInt16($bytes,$s+4)
      for($i=0;$i -lt $count;$i++) { $n=[BitConverter]::ToUInt16($bytes,$cursor+2); if($bytes[$cursor] -eq 18){$aces += (Slice $cursor $n)}else{$otherAces += (Slice $cursor $n)}; $cursor += $n }
    }
    return @{control=[BitConverter]::ToUInt16($bytes,2);sacl_non_ace18=$otherAces;sacl_header=$(if($s){Slice $s 8}else{''});owner=$(if($o){Slice $o (8+4*$bytes[$o+1])}else{''});group=$(if($g){Slice $g (8+4*$bytes[$g+1])}else{''});dacl=$(if($d){Slice $d ([BitConverter]::ToUInt16($bytes,$d+2))}else{''});ace18=$aces;raw_selected_sd=(Hex $bytes)}
}
[AttributeScratch]::MaximumAllowedControl=[bool]$MaximumAllowedControl
$privilegesBefore=(& whoami.exe /all | Out-String)
[AttributeScratch]::EnableSecurityPrivilege()
$privilegesAfter=(& whoami.exe /all | Out-String)
$root = Join-Path $drive.FullName ('ace18-scratch-'+[Guid]::NewGuid().ToString('N'))
if(Test-Path -LiteralPath $root){throw 'Scratch collision'}
New-Item -ItemType Directory -Path $root | Out-Null
$results=@()
function Unhex([string]$text) { if($text.Length % 2){throw 'Invalid hex'}; [byte[]]$value=for($j=0;$j -lt $text.Length;$j+=2){[Convert]::ToByte($text.Substring($j,2),16)}; return ,$value }
# Avoid PowerShell pipeline flattening byte arrays.
$challenges = [System.Collections.Generic.List[byte[]]]::new()
foreach($item in $inputData.aces){$challenges.Add((Unhex $item.ace_hex))}
$challenges.Add([AttributeScratch]::Construct())
for($i=0;$i -lt $challenges.Count;$i++) {
 $path=Join-Path $root ('case-'+$i); if($ScratchObjectType -eq 'Directory'){New-Item -ItemType Directory -Path $path | Out-Null;$child=Join-Path $path 'child.txt'}else{[IO.File]::WriteAllText($path,'scratch-only');$child=Join-Path $root ('sibling-'+$i+'.txt')};[IO.File]::WriteAllText($child,'scratch-only')
 $h=[AttributeScratch]::Open($child,$false);try{$childBefore=Parse-SD ([AttributeScratch]::Query($h))}finally{$h.Dispose()}
 $h=[AttributeScratch]::Open($path,$false); try{$before=Parse-SD ([AttributeScratch]::Query($h));$negative=[AttributeScratch]::Apply($h,$challenges[$i])}finally{$h.Dispose()}
 $h=[AttributeScratch]::Open($path,$false);try{$afterNegative=Parse-SD ([AttributeScratch]::Query($h))}finally{$h.Dispose()}
 $h=[AttributeScratch]::Open($path,$true);try{$status=[AttributeScratch]::Apply($h,$challenges[$i])}finally{$h.Dispose()}
 $h=[AttributeScratch]::Open($path,$false);try{$after=Parse-SD ([AttributeScratch]::Query($h))}finally{$h.Dispose()}
 $h=[AttributeScratch]::Open($child,$false);try{$childAfter=Parse-SD ([AttributeScratch]::Query($h))}finally{$h.Dispose()}
 $preserved=($before.owner -eq $after.owner -and $before.group -eq $after.group -and $before.dacl -eq $after.dacl)
 $controlPreserved=($before.control -eq $after.control)
 $otherSaclPreserved=((ConvertTo-Json -Compress -InputObject @($before.sacl_non_ace18)) -eq (ConvertTo-Json -Compress -InputObject @($after.sacl_non_ace18)))
 $childNonAttributePreserved=($childBefore.owner -eq $childAfter.owner -and $childBefore.group -eq $childAfter.group -and $childBefore.dacl -eq $childAfter.dacl -and $childBefore.control -eq $childAfter.control -and ((ConvertTo-Json -Compress -InputObject @($childBefore.sacl_non_ace18)) -eq (ConvertTo-Json -Compress -InputObject @($childAfter.sacl_non_ace18))))
 $siblingEntirePreserved=($childBefore.raw_selected_sd -eq $childAfter.raw_selected_sd)
 $exact=($after.ace18.Count -eq 1 -and $after.ace18[0] -eq (Hex $challenges[$i]))
 $results += @{case=$i;path=$path;expected_ace=(Hex $challenges[$i]);before=$before;negative_status=$negative;negative_unchanged=($before.raw_selected_sd -eq $afterNegative.raw_selected_sd);after_negative=$afterNegative;set_status=$status;after_reopen=$after;child_before=$childBefore;child_after=$childAfter;owner_group_dacl_preserved=$preserved;control_preserved=$controlPreserved;non_ace18_sacl_preserved=$otherSaclPreserved;child_non_attribute_invariants_preserved=$childNonAttributePreserved;sibling_entire_descriptor_preserved=$siblingEntirePreserved;intended_inheritance_observation='Directory child ACE18 propagation recorded separately; unrelated changes remain failures';ace_exact=$exact;passed=($negative -eq 5 -and $before.raw_selected_sd -eq $afterNegative.raw_selected_sd -and $status -eq 0 -and $preserved -and $controlPreserved -and $otherSaclPreserved -and $childNonAttributePreserved -and ($ScratchObjectType -ne 'File' -or $siblingEntirePreserved) -and $exact)}
}
$report=@{schema=1;scratch_object_type=$ScratchObjectType;maximum_allowed_control=[bool]$MaximumAllowedControl;supported_api='SetSecurityInfo ATTRIBUTE only; no SetKernelObjectSecurity';executed=$true;scratch_root=$root;source_inventory_sha256=$inputData.source_inventory_sha256;input_sha256=(Get-FileHash -LiteralPath $InputJson -Algorithm SHA256).Hash;os_version=[Environment]::OSVersion.Version.ToString();privileges_before=$privilegesBefore;privileges_after=$privilegesAfter;descriptor_scope='OWNER|GROUP|DACL|SACL|LABEL|ATTRIBUTE full raw descriptor queried; setter ATTRIBUTE only';negative_scope='same caller, handle without WRITE_DAC; not an unelevated-token control';results=$results;all_passed=(@($results|Where-Object {-not $_.passed}).Count -eq 0)}
$json=$report|ConvertTo-Json -Depth 20
$stream=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew);try{$writer=[IO.StreamWriter]::new($stream);$writer.Write($json);$writer.Flush()}finally{$stream.Dispose()}
if(-not $report.all_passed){throw 'Scratch gate failed; evidence preserved'}
