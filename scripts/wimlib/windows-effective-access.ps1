# Independent Windows AccessCheck plus real impersonated no-follow reads.
# Dot-source this file; pass an existing disposable admin credential. No accounts
# or file metadata are created or changed. Passwords are never returned.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;
public static class EffectiveAccessProbe {
 [StructLayout(LayoutKind.Sequential)] struct Luid { public uint low; public int high; }
 [StructLayout(LayoutKind.Sequential)] struct Priv { public uint count; public Luid luid; public uint attributes; }
 [StructLayout(LayoutKind.Sequential)] struct Mapping { public uint read,write,execute,all; }
 [StructLayout(LayoutKind.Sequential)] struct Status { public IntPtr status; public UIntPtr bytes; }
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sd,uint d,uint f,IntPtr t);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr p,uint access,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid luid);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AdjustTokenPrivileges(IntPtr token,bool disable,ref Priv p,uint length,IntPtr previous,IntPtr needed);
 [DllImport("advapi32.dll",SetLastError=true,EntryPoint="AdjustTokenPrivileges")] static extern bool DisablePrivileges(IntPtr token,bool disable,IntPtr p,uint length,IntPtr previous,IntPtr needed);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool DuplicateTokenEx(IntPtr token,uint access,IntPtr attributes,int level,int type,out IntPtr duplicate);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LogonUser(string user,string domain,string password,uint type,uint provider,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(IntPtr token,int kind,IntPtr data,uint length,out uint needed);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool AccessCheck(byte[] sd,IntPtr token,uint requested,ref Mapping mapping,byte[] privileges,ref uint privilegeLength,out uint granted,out bool allowed);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool ImpersonateLoggedOnUser(IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool RevertToSelf();
 [DllImport("ntdll.dll")] static extern int NtQuerySecurityObject(SafeFileHandle h,uint flags,byte[] sd,uint length,out uint needed);
 static void Error(){throw new Win32Exception(Marshal.GetLastWin32Error());}
 static void EnableBackup(){IntPtr t;if(!OpenProcessToken(GetCurrentProcess(),0x28,out t))Error();try{foreach(string name in new[]{"SeBackupPrivilege","SeSecurityPrivilege"}){Priv p=new Priv();p.count=1;p.attributes=2;if(!LookupPrivilegeValue(null,name,out p.luid)||!AdjustTokenPrivileges(t,false,ref p,0,IntPtr.Zero,IntPtr.Zero))Error();if(Marshal.GetLastWin32Error()==1300)throw new Exception(name+" not assigned");}}finally{CloseHandle(t);}}
 static byte[] Descriptor(string path){EnableBackup();using(var h=CreateFile(path,0x01020000,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){if(h.IsInvalid)Error();byte[] b=new byte[65536];uint n;int e=NtQuerySecurityObject(h,0x1ff,b,(uint)b.Length,out n);if(e<0)throw new Exception("NtQuerySecurityObject "+e);Array.Resize(ref b,(int)n);return b;}}
 public sealed class Result { public string Context,UserSid;public bool Administrator,PrivilegesDisabled,AccessCheckAllowsRead,ActualReadHandleOpened;public uint GrantedAccess,SourceGrantedAccess;public int ReadError;public bool SourceAccessCheckAllowsRead,SourceDescriptorProvided,DescriptorExact,PolicyEquivalent;public string SourceDescriptorHex,InstalledDescriptorHex; }
 static Result Check(string path,byte[] sd,IntPtr token,string context,byte[] source){IntPtr duplicate;if(!DuplicateTokenEx(token,0x2c,IntPtr.Zero,2,2,out duplicate))Error();try{if(!DisablePrivileges(duplicate,true,IntPtr.Zero,0,IntPtr.Zero,IntPtr.Zero))Error();Mapping m=new Mapping{read=0x120089,write=0x120116,execute=0x1200a0,all=0x1f01ff};uint n=1024,granted;bool allowed;byte[] privileges=new byte[n];if(!AccessCheck(sd,duplicate,0x120089,ref m,privileges,ref n,out granted,out allowed))Error();var identity=new WindowsIdentity(duplicate);var r=new Result{Context=context,UserSid=identity.User.Value,Administrator=new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator),PrivilegesDisabled=true,AccessCheckAllowsRead=allowed,GrantedAccess=granted};r.InstalledDescriptorHex=BitConverter.ToString(sd);if(source!=null){n=1024;bool sourceAllowed;uint sourceGranted;byte[] sourcePrivileges=new byte[n];if(!AccessCheck(source,duplicate,0x120089,ref m,sourcePrivileges,ref n,out sourceGranted,out sourceAllowed))Error();r.SourceDescriptorProvided=true;r.SourceAccessCheckAllowsRead=sourceAllowed;r.SourceGrantedAccess=sourceGranted;r.SourceDescriptorHex=BitConverter.ToString(source);r.DescriptorExact=r.SourceDescriptorHex==r.InstalledDescriptorHex;r.PolicyEquivalent=sourceAllowed==allowed&&sourceGranted==granted;}if(!ImpersonateLoggedOnUser(duplicate))Error();try{using(var h=CreateFile(path,0x80000000,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){r.ActualReadHandleOpened=!h.IsInvalid;r.ReadError=h.IsInvalid?Marshal.GetLastWin32Error():0;}}finally{if(!RevertToSelf())Error();}return r;}finally{CloseHandle(duplicate);}}
 public static Result[] Probe(string path,string user,string password){return ProbeWithSource(path,user,password,null);}public static Result[] ProbeWithSource(string path,string user,string password,byte[] source){byte[] sd=Descriptor(path);IntPtr current;if(!OpenProcessToken(GetCurrentProcess(),0xa,out current))Error();var results=new List<Result>();try{results.Add(Check(path,sd,current,"current-system",source));}finally{CloseHandle(current);}IntPtr logon;if(!LogonUser(user,".",password,2,0,out logon))Error();try{IntPtr buffer=Marshal.AllocHGlobal(IntPtr.Size);try{uint n;if(!GetTokenInformation(logon,19,buffer,(uint)IntPtr.Size,out n))Error();IntPtr linked=Marshal.ReadIntPtr(buffer);try{var a=Check(path,sd,logon,"interactive-logon",source);var b=Check(path,sd,linked,"linked-uac-token",source);if(a.Administrator==b.Administrator)throw new Exception("expected distinct admin and filtered standard token");a.Context=a.Administrator?"administrator":"filtered-standard";b.Context=b.Administrator?"administrator":"filtered-standard";results.Add(a);results.Add(b);}finally{CloseHandle(linked);}}finally{Marshal.FreeHGlobal(buffer);}}finally{CloseHandle(logon);}return results.ToArray();}
}
'@
function Test-EffectiveWindowsAccess {
    param([Parameter(Mandatory)][string]$Path,
          [Parameter(Mandatory)][System.Management.Automation.PSCredential]$Credential)
    [EffectiveAccessProbe]::Probe($Path, $Credential.UserName,
                                 $Credential.GetNetworkCredential().Password)
}
