# Host-only AST/C# guard tests. No Windows API calls.
$ErrorActionPreference='Stop'
$p=Join-Path $PSScriptRoot 'windows-alias-basic4-two-cycle-scratch.ps1';$errors=$null;$tokens=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($p,[ref]$tokens,[ref]$errors);if($errors.Count){throw $errors}
$s=Get-Content $p -Raw;$cs=$s.Split("Add-Type -TypeDefinition @'`n")[1].Split("`n'@")[0];Add-Type $cs
foreach($name in @('ValidateRoot','Same','SnapshotSame')){$f=$ast.Find({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq$name},$true);if(-not$f){throw 'Pure guard missing'};Invoke-Expression $f.Extent.Text}
$root='D:\alias-basic4-cycle-0123456789abcdef0123456789abcdef'
if((ValidateRoot $root 'D:\')-cne('\\?\'+$root)){throw 'Literal extended root changed'}
foreach($bad in @('C:\alias-basic4-cycle-0123456789abcdef0123456789abcdef',($root+'\..'),($root+'-other'),($root+'\file'),($root.Replace('\','/')))){$rejected=$false;try{ValidateRoot $bad 'D:\'|Out-Null}catch{$rejected=$true};if(-not$rejected){throw 'Unbound root accepted'}}
$expected='{"basic36":"abcd","file_id_info24":"1234","security_raw":"5678","object_id_full64":""}'|ConvertFrom-Json
$a=@{basic36='abcd';file_id_info24='1234';security_raw='5678';object_id_full64=''};if(-not(Same $a $expected)){throw 'JSON own snapshot not compared by properties'}
foreach($f in @('basic36','file_id_info24','security_raw','object_id_full64')){$b=$a.Clone();$b[$f]='changed';if(Same $b $expected){throw 'Changed metadata accepted'};$b.Remove($f);if(Same $b $expected){throw 'Missing metadata accepted'}}
$basic=[byte[]]::new(40);foreach($o in @(0,8,16,24)){[BitConverter]::GetBytes([Int64]134000000000000000).CopyTo($basic,$o)};[BitConverter]::GetBytes([UInt32]32).CopyTo($basic,32);[AliasBasic4Cycle]::ValidateBasic($basic)
foreach($o in @(0,8,16,24)){foreach($sentinel in @(0,-1,-2)){$bad=[byte[]]$basic.Clone();[BitConverter]::GetBytes([Int64]$sentinel).CopyTo($bad,$o);$rejected=$false;try{[AliasBasic4Cycle]::ValidateBasic($bad)}catch{$rejected=$true};if(-not$rejected){throw 'Literal zero/sentinel accepted'}}}
if($cs.Contains('0x900c0')-or$cs.Contains('FSCTL_SET_OBJECT_ID')){throw 'Unexpected ObjectID creator/setter'}
'Host compile, root confinement, JSON metadata comparisons and exact fourtime sentinel guards pass; no Windows execution'
