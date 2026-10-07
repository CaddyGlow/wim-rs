# Host-only exact restore input guard tests; no Windows calls or target setters.
$ErrorActionPreference='Stop'
$p=Join-Path $PSScriptRoot 'windows-object-id-basic4-scratch.ps1';$errors=$null;$tokens=$null
[System.Management.Automation.Language.Parser]::ParseFile($p,[ref]$tokens,[ref]$errors)|Out-Null
if($errors.Count){throw 'Scratch helper syntax errors'}
$s=Get-Content -LiteralPath $p -Raw;$source=$s.Split("Add-Type -TypeDefinition @'`n")[1].Split("`n'@")[0]
Add-Type -TypeDefinition $source
$valid=[byte[]]::new(40);foreach($offset in @(0,8,16,24)){[BitConverter]::GetBytes([Int64](134000000000000000+$offset)).CopyTo($valid,$offset)};[BitConverter]::GetBytes([UInt32]32).CopyTo($valid,32)
[ObjectIdBasic4Scratch]::ValidateBasic($valid)
foreach($offset in @(0,8,16,24)){foreach($sentinel in @([Int64]0,[Int64]-1,[Int64]-2)){$bad=[byte[]]$valid.Clone();[BitConverter]::GetBytes($sentinel).CopyTo($bad,$offset);$rejected=$false;try{[ObjectIdBasic4Scratch]::ValidateBasic($bad)}catch{$rejected=$true};if(-not$rejected){throw 'Zero/sentinel accepted as literal exact restoration'}}}
$bad=[byte[]]$valid.Clone();[Array]::Clear($bad,32,4);$rejected=$false;try{[ObjectIdBasic4Scratch]::ValidateBasic($bad)}catch{$rejected=$true};if(-not$rejected){throw 'Zero attributes accepted as exact restoration'}
$rejected=$false;try{[ObjectIdBasic4Scratch]::ValidateBasic([byte[]]::new(36))}catch{$rejected=$true};if(-not$rejected){throw 'Incomplete native buffer accepted'}
$padding=[byte[]]$valid.Clone();for($i=36;$i-lt40;$i++){$padding[$i]=255};[ObjectIdBasic4Scratch]::ValidateBasic($padding)
'Positive exact basic4 guard passes; all four zero/-1/-2 sentinels, zero attributes and truncated buffer fail closed; padding ignored'
