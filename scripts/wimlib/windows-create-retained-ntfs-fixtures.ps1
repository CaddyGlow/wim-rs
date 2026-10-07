# Format only TWO NEW dedicated disks; preserve receipts/files for clean offline tests.
param([Parameter(Mandatory=$true)][string]$ReportPath)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-native-ads-io.ps1')
if(Test-Path -LiteralPath $ReportPath){throw 'Existing receipt forbidden'}
$receipt=@{schema=1;scope='Windows-created isolated per-directory case fixture and partitioned many-ADS fixture; no source/evidence disk changes';started_utc=[DateTime]::UtcNow.ToString('o');kernel=[Environment]::OSVersion.Version.ToString();identity=(& whoami.exe /all|Out-String);disks=@();case=$null;ads=$null;completed=$false;error=$null}
try{
 $specs=@(@{serial='V12-CASE-FIXTURE';letter='V';label='NtfsCaseFixture'},@{serial='V12-ADS-FIXTURE';letter='W';label='NtfsAdsFixture'})
 # Validate both identities BEFORE the first mutation; no disk numbers assumed.
 $selected=@()
 foreach($spec in $specs){
  $candidate=@(Get-Disk|Where-Object {([string]$_.SerialNumber).Trim() -eq $spec.serial})
  if($candidate.Count -ne 1){throw ('Dedicated serial missing/ambiguous: '+$spec.serial)}
  $disk=$candidate[0]
  if($disk.Number -eq 0 -or $disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 536870912 -or $disk.PartitionStyle -ne 'RAW'){throw ('New blank512MiB disk guard failed: '+$spec.serial)}
  if(Get-Volume -DriveLetter $spec.letter -ErrorAction SilentlyContinue){throw ('Drive occupied: '+$spec.letter)}
  $selected+=@{spec=$spec;disk=$disk}
 }
 if($selected[0].disk.Number -eq $selected[1].disk.Number){throw 'Dedicated disks must differ'}
 foreach($item in $selected){
  $spec=$item.spec;$disk=$item.disk
  $receipt.disks+=@{serial=$spec.serial;number=$disk.Number;virtual_bytes=$disk.Size;before_style=[string]$disk.PartitionStyle;is_boot=$disk.IsBoot;is_system=$disk.IsSystem;requested_letter=$spec.letter}
  # MBR ensures NTFS partition1 for tests; no GPT MSR partition can shift index.
  $disk|Initialize-Disk -PartitionStyle MBR -PassThru|New-Partition -UseMaximumSize -DriveLetter $spec.letter|Format-Volume -FileSystem NTFS -NewFileSystemLabel $spec.label -Confirm:$false|Out-Null
  $parts=@(Get-Partition -DiskNumber $disk.Number)
  if($parts.Count -ne 1 -or $parts[0].PartitionNumber -ne 1 -or $parts[0].DriveLetter -ne $spec.letter){throw 'Unexpected fixture partition layout'}
  $receipt.disks[-1].after_style=[string](Get-Disk -Number $disk.Number).PartitionStyle
  $receipt.disks[-1].after_partitions=@($parts|Select-Object DiskNumber,PartitionNumber,Offset,Size,DriveLetter,Type)
  $receipt.disks[-1].after_volume=($parts[0]|Get-Volume|Select-Object FileSystem,DriveLetter,Size,FileSystemLabel)
 }
 $case='V:\case';$control='V:\control'
 New-Item -ItemType Directory -Path $case|Out-Null
 New-Item -ItemType Directory -Path $control|Out-Null
 if(@(Get-ChildItem -LiteralPath $case -Force).Count -ne 0){throw 'Case directory must be empty'}
 $enable=& fsutil.exe file setCaseSensitiveInfo $case enable 2>&1|Out-String;$enableExit=$LASTEXITCODE
 $query=& fsutil.exe file queryCaseSensitiveInfo $case 2>&1|Out-String;$queryExit=$LASTEXITCODE
 $controlQuery=& fsutil.exe file queryCaseSensitiveInfo $control 2>&1|Out-String;$controlExit=$LASTEXITCODE
 $receipt.case=@{path=$case;empty=(@(Get-ChildItem -LiteralPath $case -Force).Count -eq 0);enable_exit=$enableExit;enable_output=$enable;query_exit=$queryExit;query_output=$query;control_path=$control;control_query_exit=$controlExit;control_query_output=$controlQuery;control_attributes=[int](Get-Item -LiteralPath $control).Attributes;global_policy_changed=$false}
 if($enableExit -ne 0 -or $queryExit -ne 0 -or $controlExit -ne 0 -or -not $receipt.case.empty){throw 'Per-directory case creation/query failed; no global feature/policy changes attempted'}
 if($receipt.case.control_attributes -ne 16){throw 'Ordinary control attributes must be DIRECTORY only'}
 New-Item -ItemType Directory -Path 'W:\OddFixture'|Out-Null
 $file='W:\OddFixture\many-ads.bin';[RetainedAdsIo]::VerifyOrCreate($file,170)|Out-Null
 $streams=@()
 for($i=0;$i -lt 160;$i++){
  $name=('stream-{0:D3}' -f $i);$path=$file+':'+$name
  $actual=[RetainedAdsIo]::VerifyOrCreate($path,[byte]$i)
  $streams+=@{name=$name;length=1;byte=$actual}
 }
 $enumerated=@(Get-Item -LiteralPath $file -Stream '*'|Select-Object Stream,Length)
 $receipt.ads=@{path=$file;partition=1;unnamed_byte=170;count=160;verified_streams=$streams;windows_stream_enumeration=$enumerated}
 $receipt.completed=$true
}catch{$receipt.error=$_.Exception.Message}finally{
 $receipt.finished_utc=[DateTime]::UtcNow.ToString('o')
 $output=[IO.File]::Open($ReportPath,[IO.FileMode]::CreateNew)
 try{$writer=[IO.StreamWriter]::new($output);$writer.Write(($receipt|ConvertTo-Json -Depth 20));$writer.Flush()}finally{$output.Dispose()}
}
if(-not $receipt.completed){throw ('Windows fixture creation failed; receipt preserved: '+$receipt.error)}
