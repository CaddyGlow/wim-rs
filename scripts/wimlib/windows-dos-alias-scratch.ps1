param([Parameter(Mandatory=$true)][string]$Parent, [switch]$CompileOnly)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class AliasScratchProbe {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr sa,uint mode,uint flags,IntPtr template);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetFileShortNameW(SafeFileHandle handle,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,byte[] output,uint length);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateHardLinkW(string name,string existing,IntPtr sa);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool GetFileSecurityW(string path,uint info,byte[] output,uint length,out uint needed);
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
if($CompileOnly){return}
if(-not [IO.Path]::IsPathRooted($Parent) -or $Parent -notmatch '^[A-Za-z]:\\' -or $Parent.Contains('..')){throw 'Parent must be an existing local absolute scratch directory'}
$item=Get-Item -LiteralPath $Parent
if(-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Parent must be a real directory'}
# A new random child is the only mutation scope; never accept an existing target tree.
$root=Join-Path $item.FullName ('QCOW2-AliasScratch-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$actions=[Collections.Generic.List[object]]::new()
function Utf16([string]$text){[BitConverter]::ToString([Text.Encoding]::Unicode.GetBytes($text)).Replace('-','')}
function Observe([string]$path){
 [ordered]@{Path=$path;PathUtf16=Utf16 $path;Identity24=[AliasScratchProbe]::Identity($path);SecurityOwnerGroupDacl=[AliasScratchProbe]::Security($path);BasicAndStandardRaw=[AliasScratchProbe]::BasicAndStandard($path);PayloadSHA256=$(if([IO.File]::Exists($path)){(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}else{$null})}
}
function SetAlias([string]$path,[string]$alias){
 if(-not $path.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Mutation outside new scratch child'}
 $before=Observe $path;$status=[AliasScratchProbe]::Set($path,$alias);$after=Observe $path
 $actions.Add([ordered]@{Path=$path;Alias=$alias;AliasUtf16=Utf16 $alias;Win32=$status;Before=$before;After=$after})
 return $status
}
function Mapping([string]$long,[string]$alias){
 $short=Join-Path ([IO.Path]::GetDirectoryName($long)) $alias
 [ordered]@{Long=$long;Alias=$alias;LongUtf16=Utf16 $long;AliasUtf16=Utf16 $alias;LongID=[AliasScratchProbe]::Identity($long);ShortID=[AliasScratchProbe]::Identity($short)}
}
$pair=Join-Path $root 'pair';[void][IO.Directory]::CreateDirectory($pair)
$a=Join-Path $pair 'FaceProcessor.dll';$b=Join-Path $pair 'FaceProcessorCore.dll'
[IO.File]::WriteAllBytes($a,[byte[]](1,2,3,4));[IO.File]::WriteAllBytes($b,[byte[]](5,6,7,8))
$initial=@(Observe $a;Observe $b)
$setupStatuses=@(SetAlias $a '';SetAlias $b '';SetAlias $a 'FACEPR~1.DLL';SetAlias $b 'FACEPR~2.DLL')
$collision=SetAlias $a 'FACEPR~2.DLL'
$collisionMapping=@(Mapping $a 'FACEPR~1.DLL';Mapping $b 'FACEPR~2.DLL')
$permutationStatuses=@(SetAlias $a '';SetAlias $b '';SetAlias $a 'FACEPR~2.DLL';SetAlias $b 'FACEPR~1.DLL')
$permutationMapping=@(Mapping $a 'FACEPR~2.DLL';Mapping $b 'FACEPR~1.DLL')
$blocker=Join-Path $pair 'BLOCK.DLL';[IO.File]::WriteAllBytes($blocker,[byte[]](9))
$longNameCollision=SetAlias $a 'BLOCK.DLL'
$hard=Join-Path $root 'hardlinks';[void][IO.Directory]::CreateDirectory($hard)
$other=Join-Path $root 'other';[void][IO.Directory]::CreateDirectory($other)
$h1=Join-Path $hard 'HardlinkedOne.bin';$h2=Join-Path $hard 'HardlinkedTwo.bin';$h3=Join-Path $other 'HardlinkedThree.bin'
[IO.File]::WriteAllBytes($h1,[byte[]](10,11,12));[AliasScratchProbe]::Link($h2,$h1);[AliasScratchProbe]::Link($h3,$h1)
$hardlinkInitial=@(Observe $h1;Observe $h2;Observe $h3)
$hardlinkStatuses=@(SetAlias $h1 '';SetAlias $h2 '';SetAlias $h3 '';SetAlias $h1 'HARD~1.BIN';SetAlias $h2 'HARD~2.BIN';SetAlias $h3 'HARD~3.BIN')
$hardlinkMappings=@()
$hp=@($h1,$h2,$h3);$ha=@('HARD~1.BIN','HARD~2.BIN','HARD~3.BIN')
for($i=0;$i-lt 3;$i++){if($hardlinkStatuses[$i+3]-eq 0){$hardlinkMappings+=Mapping $hp[$i] $ha[$i]}}
function ParentEnumeration([string]$parent){
 @([AliasScratchProbe]::Enumerate($parent)|ForEach-Object{
  $long=Join-Path $parent $_.Name
  [ordered]@{Parent=$parent;LongName=$_.Name;LongNameUtf16=Utf16 $_.Name;Alias=$_.ShortName;AliasUtf16=Utf16 $_.ShortName;LongID=[AliasScratchProbe]::Identity($long);AliasID=$(if($_.ShortName){[AliasScratchProbe]::Identity((Join-Path $parent $_.ShortName))}else{$null})}
 })
}
$hardlinkOrderingRounds=@()
for($first=0;$first-lt 3;$first++){
 $roundBefore=@(Observe $h1;Observe $h2;Observe $h3)
 $clear=@(SetAlias $h1 '';SetAlias $h2 '';SetAlias $h3 '')
 $emptyEnum=@(ParentEnumeration $hard;ParentEnumeration $other)
 $order=@($first)+@(0..2|Where-Object{$_-ne $first})
 $sets=@();foreach($index in $order){$sets+=[ordered]@{LinkIndex=$index;Status=SetAlias $hp[$index] $ha[$index]}}
 $hardlinkOrderingRounds+=[ordered]@{FirstLink=$first;Before=$roundBefore;ClearStatuses=$clear;EmptyParentEnumeration=$emptyEnum;SetResults=$sets;FinalParentEnumeration=@(ParentEnumeration $hard;ParentEnumeration $other);After=@(Observe $h1;Observe $h2;Observe $h3)}
}
$directory=Join-Path $root 'LongDirectoryChallenge';[void][IO.Directory]::CreateDirectory($directory)
$directoryStatus=SetAlias $directory 'LONGDI~1'
$directoryMapping=$(if($directoryStatus-eq 0){Mapping $directory 'LONGDI~1'}else{$null})
$privileges=(& whoami.exe /priv 2>&1|Out-String)
$report=[ordered]@{Schema=1;Scope='New isolated scratch only; candidate API evidence, not installed restoration';Root=$root;RootUtf16=Utf16 $root;WindowsBuild=[Environment]::OSVersion.Version.ToString();Privileges=$privileges;SecurityScope='Owner/group/DACL only; no SACL preservation claim';Initial=$initial;SetupStatuses=$setupStatuses;CollisionStatus=$collision;CollisionMapping=$collisionMapping;PermutationStatuses=$permutationStatuses;PermutationMapping=$permutationMapping;LongNameCollisionStatus=$longNameCollision;HardlinkInitial=$hardlinkInitial;HardlinkStatuses=$hardlinkStatuses;HardlinkOrderingRounds=$hardlinkOrderingRounds;HardlinkMappings=$hardlinkMappings;HardlinkFinal=@(Observe $h1;Observe $h2;Observe $h3);DirectoryStatus=$directoryStatus;DirectoryMapping=$directoryMapping;Actions=$actions}
$report|ConvertTo-Json -Depth 12
