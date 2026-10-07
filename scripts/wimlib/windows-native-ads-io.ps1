# Win32 stream paths avoid .NET Framework's alternate-stream path validation.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class RetainedAdsIo {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool WriteFile(SafeFileHandle h,byte[] b,uint count,out uint done,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool ReadFile(SafeFileHandle h,byte[] b,uint count,out uint done,IntPtr overlapped);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out Info info);
 [StructLayout(LayoutKind.Sequential)] struct Info {public uint Attributes;public uint CreationLow,CreationHigh,AccessLow,AccessHigh,WriteLow,WriteHigh,VolumeSerial,SizeHigh,SizeLow,Links,IndexHigh,IndexLow;}
 static SafeFileHandle Open(string path,uint access,uint disposition){var h=CreateFileW(path,access,7,IntPtr.Zero,disposition,0x00200000,IntPtr.Zero);if(h.IsInvalid){int error=Marshal.GetLastWin32Error();h.Dispose();throw new Win32Exception(error);}Info info;if(!GetFileInformationByHandle(h,out info)){int error=Marshal.GetLastWin32Error();h.Dispose();throw new Win32Exception(error);}if((info.Attributes&0x400)!=0){h.Dispose();throw new InvalidOperationException("Reparse handle forbidden");}return h;}
 public static byte VerifyExisting(string path,byte expected){using(var r=Open(path,0x80000000,3)){uint done;var bytes=new byte[2];if(!ReadFile(r,bytes,2,out done,IntPtr.Zero))throw new Win32Exception(Marshal.GetLastWin32Error());if(done!=1||bytes[0]!=expected)throw new InvalidOperationException("Existing stream differs; no overwrite permitted");return bytes[0];}}
 public static byte VerifyOrCreate(string path,byte expected){bool created=false;SafeFileHandle h;try{h=Open(path,0x40000000,1);created=true;}catch(Win32Exception e){if(e.NativeErrorCode!=80&&e.NativeErrorCode!=183)throw;h=null;}if(created){using(h){uint done;if(!WriteFile(h,new byte[]{expected},1,out done,IntPtr.Zero)||done!=1)throw new Win32Exception(Marshal.GetLastWin32Error());}}using(var r=Open(path,0x80000000,3)){uint done;var bytes=new byte[2];if(!ReadFile(r,bytes,2,out done,IntPtr.Zero))throw new Win32Exception(Marshal.GetLastWin32Error());if(done!=1||bytes[0]!=expected)throw new InvalidOperationException("Existing stream differs; no overwrite permitted");return bytes[0];}}
}
'@
