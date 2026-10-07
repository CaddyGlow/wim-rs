# Read-only fixture observations on an explicitly selected disposable Windows guest.
# Dot-source; call Get-CaptureRuntimeMetadata. Does not create or repair metadata.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CaptureRuntimeProbe {
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
 static extern SafeFileHandle CreateFile(string p,uint access,uint share,IntPtr security,uint mode,uint flags,IntPtr template);
 [DllImport("kernel32.dll", SetLastError=true)]
 static extern bool DeviceIoControl(SafeFileHandle h,uint code,byte[] input,uint ilen,byte[] output,uint olen,out uint returned,IntPtr overlap);
 public static byte[] ReadControl(string path,uint code,byte[] input) {
  using(var h=CreateFile(path,0x80000000,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   byte[] output=new byte[65536];uint n;
   if(!DeviceIoControl(h,code,input,(uint)(input==null?0:input.Length),output,(uint)output.Length,out n,IntPtr.Zero))
    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
   Array.Resize(ref output,(int)n);return output;
  }
 }
 public static long[][] AllocatedRanges(string path,long length) {
  byte[] input=new byte[16];Array.Copy(BitConverter.GetBytes(length),0,input,8,8);
  byte[] bytes=ReadControl(path,0x940cf,input);
  if(bytes.Length%16!=0)throw new InvalidDataException("misaligned allocated range result");
  long[][] result=new long[bytes.Length/16][];
  for(int i=0;i<result.Length;i++)result[i]=new long[]{BitConverter.ToInt64(bytes,i*16),BitConverter.ToInt64(bytes,i*16+8)};
  return result;
 }
}
'@

function Get-CaptureRuntimeMetadata {
    param([string]$Root='C:\NativeDiskCapture')
    $ErrorActionPreference='Stop'
    $rows=@(Get-ChildItem -LiteralPath $Root -Force | ForEach-Object {
        $reparse=$null
        if (($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            $reparse=[BitConverter]::ToString([CaptureRuntimeProbe]::ReadControl($_.FullName,0x900a8,$null))
        }
        $ranges=$null
        if (($_.Attributes -band [IO.FileAttributes]::SparseFile) -ne 0) {
            $ranges=@([CaptureRuntimeProbe]::AllocatedRanges($_.FullName,$_.Length) | ForEach-Object {
                [PSCustomObject]@{Offset=$_[0];Length=$_[1]}
            })
        }
        [PSCustomObject]@{Name=$_.Name;Attributes=[int]$_.Attributes;RawReparse=$reparse;AllocatedRanges=$ranges}
    })
    # Follow each internal link with an ordinary file read, proving its actual
    # destination. The exhaustive stream inventory separately opens no-follow.
    $links=@('internal.junction','relative.link' | ForEach-Object {
        $path=Join-Path $Root $_
        if ((Get-Item -LiteralPath $path -Force).PSIsContainer) {$path=Join-Path $path 'payload.bin'}
        [PSCustomObject]@{Name=$_;ResolvedPath=$path;SHA256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
    })
    [PSCustomObject]@{Root=$Root;Rows=$rows;LinkReads=$links}
}
