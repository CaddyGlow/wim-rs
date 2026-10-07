param([Parameter(Mandatory=$true)][string]$Parent,[switch]$CompileOnly)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class AliasCreationProbe {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr sa,uint mode,uint flags,IntPtr template);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetFileShortNameW(SafeFileHandle handle,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,byte[] output,uint length);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateHardLinkW(string name,string existing,IntPtr sa);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool GetFileSecurityW(string path,uint info,byte[] output,uint length,out uint needed);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool MoveFileExW(string oldName,string newName,uint flags);
 public static int Rename(string oldName,string newName){return MoveFileExW(oldName,newName,0)?0:Marshal.GetLastWin32Error();}
 public static int Set(string path,string name) {
  using(var h=CreateFileW(path,0x10000,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(h.IsInvalid)return Marshal.GetLastWin32Error();
   return SetFileShortNameW(h,name)?0:Marshal.GetLastWin32Error();
  }
 }
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
 public static string BasicAndStandard(string path) {
  using(var h=CreateFileW(path,0x80,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var basic=new byte[40];var standard=new byte[24];
   if(!GetFileInformationByHandleEx(h,0,basic,40)||!GetFileInformationByHandleEx(h,1,standard,24))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   return BitConverter.ToString(basic).Replace("-","")+":"+BitConverter.ToString(standard).Replace("-","");
  }
 }
 public static string Identity(string path) {
  using(var h=CreateFileW(path,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   var bytes=new byte[24];if(!GetFileInformationByHandleEx(h,18,bytes,24))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   return BitConverter.ToString(bytes).Replace("-","");
  }
 }
 public static void Link(string path,string existing) {if(!CreateHardLinkW(path,existing,IntPtr.Zero))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}
 public static string Security(string path) {
  uint n;GetFileSecurityW(path,7,null,0,out n);if(n==0||n>1048576)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  var bytes=new byte[n];if(!GetFileSecurityW(path,7,bytes,n,out n))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  return BitConverter.ToString(bytes).Replace("-","");
 }
}
'@

if($CompileOnly){Write-Output 'C# compiled; no Windows APIs executed';exit 0}
if($Parent -notmatch '^[A-Za-z]:\\$' -or $Parent.Substring(0,2) -eq $env:SystemDrive){throw 'Require dedicated non-system volume root'}
$volume=Get-Volume -DriveLetter $Parent[0]
if($volume.FileSystem -ne 'NTFS' -or $volume.FileSystemLabel -ne 'QCOW2SCRATCH'){throw 'Require explicitly labeled QCOW2SCRATCH NTFS volume'}
$global=(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem').NtfsDisable8dot3NameCreation
if($global -ne 2){throw 'Global 8dot3 policy must already be per-volume (2); no global policy change permitted'}
$queryBefore=@(& fsutil.exe 8dot3name query $Parent 2>&1);$queryBeforeStatus=$LASTEXITCODE
$enableOutput=@(& fsutil.exe 8dot3name set $Parent 0 2>&1);$enableStatus=$LASTEXITCODE
if($enableStatus -ne 0){throw 'Dedicated-volume 8dot3 enable failed'}
$queryAfter=@(& fsutil.exe 8dot3name query $Parent 2>&1);$queryAfterStatus=$LASTEXITCODE
$root=Join-Path $Parent ('QCOW2-AliasCreation-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($root)
function Utf16([string]$s){[BitConverter]::ToString([Text.Encoding]::Unicode.GetBytes($s)).Replace('-','')}
function Observe([string]$p){[ordered]@{Path=$p;PathUtf16=Utf16 $p;Identity24=[AliasCreationProbe]::Identity($p);PayloadSHA256=(Get-FileHash -LiteralPath $p).Hash;SecurityOwnerGroupDacl=[AliasCreationProbe]::Security($p);BasicAndStandardRaw=[AliasCreationProbe]::BasicAndStandard($p)}}
function Entries([string]$p){@([AliasCreationProbe]::Enumerate($p)|ForEach-Object{ $long=Join-Path $p $_.Name;[ordered]@{Parent=$p;LongName=$_.Name;LongNameUtf16=Utf16 $_.Name;Alias=$_.ShortName;AliasUtf16=Utf16 $_.ShortName;LongID=[AliasCreationProbe]::Identity($long);AliasID=$(if($_.ShortName){[AliasCreationProbe]::Identity((Join-Path $p $_.ShortName))}else{$null})}})}
$rounds=@()
foreach($first in 0,1,2){
 $case=Join-Path $root ('order'+$first);$same=Join-Path $case 'same';$other=Join-Path $case 'other';[void][IO.Directory]::CreateDirectory($same);[void][IO.Directory]::CreateDirectory($other)
 $paths=@((Join-Path $same 'FaceProcessor.dll'),(Join-Path $same 'FaceProcessorCore.dll'),(Join-Path $other 'FaceProcessorExtra.dll'))
 [IO.File]::WriteAllBytes($paths[$first],[byte[]](17,33,49,65));$initial=Observe $paths[$first];$steps=@()
 foreach($i in 0,1,2){if($i -ne $first){[AliasCreationProbe]::Link($paths[$i],$paths[$first]);$steps+= [ordered]@{CreatedIndex=$i;Observations=@(foreach($p in $paths){if([IO.File]::Exists($p)){Observe $p}});Entries=(@(Entries $same)+@(Entries $other))}}}
 $beforeRename=@(foreach($p in $paths){Observe $p});$beforeEntries=(@(Entries $same)+@(Entries $other))
 $new=Join-Path $other 'RenamedFaceProcessorExtra.dll';$renameStatus=[AliasCreationProbe]::Rename($paths[2],$new);if($renameStatus -eq 0){$paths[2]=$new}
 $rounds+=[ordered]@{FirstLink=$first;Initial=$initial;CreationSteps=$steps;BeforeRename=$beforeRename;BeforeRenameEntries=$beforeEntries;RenameStatus=$renameStatus;AfterRename=@(foreach($p in $paths){Observe $p});AfterRenameEntries=(@(Entries $same)+@(Entries $other))}
}
[ordered]@{Schema=1;Scope='New dedicated scratch volume only; no installed restoration';Root=$root;RootUtf16=Utf16 $root;GlobalPolicy=$global;QueryBefore=$queryBefore;QueryBeforeStatus=$queryBeforeStatus;EnableOutput=$enableOutput;EnableStatus=$enableStatus;QueryAfter=$queryAfter;QueryAfterStatus=$queryAfterStatus;WindowsBuild=[Environment]::OSVersion.Version.ToString();Privileges=@(& whoami /priv);Rounds=$rounds;SecurityScope='owner/group/DACL only; SACL untested'}|ConvertTo-Json -Depth 16
