param([Parameter(Mandatory=$true)][string]$PriorReceipt,[Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-native-ads-io.ps1')
if(Test-Path -LiteralPath $ReportPath){throw 'Existing receipt forbidden'}
$r=Get-Content -LiteralPath $PriorReceipt -Raw|ConvertFrom-Json
if($r.completed -or -not $r.error -or $r.disks.Count -ne 2 -or $null -ne $r.ads){throw 'Expected preserved incomplete ADS creation receipt'}
$specs=@(@{serial='V12-CASE-FIXTURE';letter='V';label='NtfsCaseFixture'},@{serial='V12-ADS-FIXTURE';letter='W';label='NtfsAdsFixture'})
foreach($s in $specs){
 $old=@($r.disks|Where-Object {$_.serial -eq $s.serial -and $_.requested_letter -eq $s.letter})
 $now=@(Get-Disk|Where-Object {([string]$_.SerialNumber).Trim() -eq $s.serial})
 if($old.Count -ne 1 -or $now.Count -ne 1){throw 'Exact two serial/letter pairs required'}
 $d=$now[0];$o=$old[0];$p=@(Get-Partition -DiskNumber $d.Number);$v=Get-Volume -DriveLetter $s.letter
 if($d.Number -ne $o.number -or $d.Number -eq 0 -or $d.IsBoot -or $d.IsSystem -or $d.Size -ne 536870912 -or $d.PartitionStyle -ne 'MBR' -or $o.before_style -ne 'RAW' -or $o.is_boot -or $o.is_system -or $p.Count -ne 1 -or $p[0].PartitionNumber -ne 1 -or $p[0].DriveLetter -ne $s.letter -or $p[0].Offset -ne $o.after_partitions[0].Offset -or $p[0].Size -ne $o.after_partitions[0].Size -or $v.FileSystem -ne 'NTFS' -or $v.FileSystemLabel -ne $s.label -or $v.Size -ne $o.after_volume.Size){throw 'Current disk/partition/label binding differs'}
}
if($r.case.path -ne 'V:\case' -or $r.case.control_path -ne 'V:\control' -or -not $r.case.empty -or $r.case.enable_exit -ne 0 -or $r.case.query_exit -ne 0 -or $r.case.control_query_exit -ne 0 -or $r.case.control_attributes -ne 16 -or $r.case.global_policy_changed){throw 'Original case evidence invalid'}
foreach($path in @('V:\case','V:\control','W:\OddFixture','W:\OddFixture\many-ads.bin')){if(-not(Test-Path -LiteralPath $path) -or ((Get-Item -LiteralPath $path -Force).Attributes -band 1024)){throw 'Expected non-reparse path missing or changed'}}
if(@(Get-ChildItem -LiteralPath 'V:\case' -Force).Count -ne 0){throw 'Case fixture changed'}
[RetainedAdsIo]::VerifyExisting('W:\OddFixture\many-ads.bin',170)|Out-Null
$r|Add-Member -NotePropertyName prior_receipt_sha256 -NotePropertyValue ((Get-FileHash -LiteralPath $PriorReceipt -Algorithm SHA256).Hash.ToLowerInvariant())
$r|Add-Member -NotePropertyName continuation_scope -NotePropertyValue 'Win32 ADS creation only; no disk formatting, no existing stream overwrite'
$r.error=$null
try{
 $streams=@();$file='W:\OddFixture\many-ads.bin'
 for($i=0;$i -lt 160;$i++){$name=('stream-{0:D3}' -f $i);$actual=[RetainedAdsIo]::VerifyOrCreate($file+':'+$name,[byte]$i);$streams+=@{name=$name;length=1;byte=$actual}}
 $r.ads=@{path=$file;partition=1;unnamed_byte=[RetainedAdsIo]::VerifyExisting($file,170);count=160;verified_streams=$streams;windows_stream_enumeration=@(Get-Item -LiteralPath $file -Stream '*'|Select-Object Stream,Length)}
 $r.completed=$true
}catch{$r.error=$_.Exception.Message;$r.completed=$false}finally{
 $r.finished_utc=[DateTime]::UtcNow.ToString('o');$out=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew)
 try{$writer=[IO.StreamWriter]::new($out);$writer.Write(($r|ConvertTo-Json -Depth 20));$writer.Flush()}finally{$out.Dispose()}
}
if(-not $r.completed){throw 'ADS continuation failed; receipt preserved'}
