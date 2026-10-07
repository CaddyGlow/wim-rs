# Read-only validation of every independently inventoried distinct DOS alias.
# Path units remain UTF-16; opening reparses does not follow their targets.
param([Parameter(Mandatory)][string]$Inventory, [string]$Output)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class InstalledAliasProbe {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
 static extern SafeFileHandle CreateFile(string path,uint access,uint share,IntPtr security,uint mode,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)]
 static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,byte[] output,uint size);
 public static string Identity(string path) {
  using(var handle=CreateFile(path,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(handle.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());
   byte[] result=new byte[24];
   if(!GetFileInformationByHandleEx(handle,18,result,24))throw new Win32Exception(Marshal.GetLastWin32Error());
   return BitConverter.ToString(result).Replace("-","");
  }
 }
 public static string Units(string hex) {
  if(hex.Length%4!=0)throw new ArgumentException("unaligned UTF-16 inventory");
  char[] units=new char[hex.Length/4];
  for(int i=0;i<units.Length;i++) {
   int lo=Convert.ToInt32(hex.Substring(i*4,2),16);
   int hi=Convert.ToInt32(hex.Substring(i*4+2,2),16);
   units[i]=(char)(lo|(hi<<8));
  }
  return new String(units);
 }
}
'@
$source=ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($Inventory))
if($source.Count -eq 0 -or $source.Count -gt 2048) {
    throw 'Inventory must contain 1 through 2048 entries; split larger inventories into verified batches'
}
$rows=@(foreach($entry in $source) {
    $relative=[InstalledAliasProbe]::Units($entry.path_utf16_le).Replace('/','\')
    $leaf=[InstalledAliasProbe]::Units($entry.alias_utf16_le)
    if(-not $relative.StartsWith('\') -or $leaf.Contains('\') -or $leaf.Contains('/')) {
        throw 'invalid absolute source path or DOS alias leaf'
    }
    $long='\\?\C:'+$relative
    $short=$long.Substring(0,$long.LastIndexOf('\')+1)+$leaf
    try {
        $longId=[InstalledAliasProbe]::Identity($long)
        $shortId=[InstalledAliasProbe]::Identity($short)
        [PSCustomObject]@{PathUTF16=$entry.path_utf16_le;AliasUTF16=$entry.alias_utf16_le;
            LongFileID=$longId;AliasFileID=$shortId;Pass=($longId-eq $shortId);Error=$null}
    } catch {
        [PSCustomObject]@{PathUTF16=$entry.path_utf16_le;AliasUTF16=$entry.alias_utf16_le;
            LongFileID=$null;AliasFileID=$null;Pass=$false;Error=$_.Exception.Message}
    }
})
$report=[PSCustomObject]@{Scope='Installed target; 24-byte FILE_ID_INFO includes volume serial and 128-bit file ID';
    InventorySHA256=(Get-FileHash -LiteralPath $Inventory -Algorithm SHA256).Hash;
    Count=$rows.Count;Failures=@($rows|Where-Object{-not $_.Pass}).Count;Rows=$rows}
$json=$report|ConvertTo-Json -Depth 5 -Compress
if($Output) {
    [IO.File]::WriteAllText($Output,$json,[Text.UTF8Encoding]::new($false))
    [PSCustomObject]@{Count=$report.Count;Failures=$report.Failures;Output=$Output;
        InventorySHA256=$report.InventorySHA256;
        OutputSHA256=(Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash}|ConvertTo-Json -Compress
} else {
    $json
}
