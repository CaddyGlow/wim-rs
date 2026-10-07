# Pure host descriptor/protection tests; no guest API or filesystem setters invoked.
param([Parameter(Mandatory=$true)][string]$InputJson)
$ErrorActionPreference='Stop'
$path=Join-Path $PSScriptRoot 'windows-reserved-trust-whole-sd-scratch.ps1';$errors=$null;$tokens=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if($errors.Count){$errors|Format-List;throw 'Syntax errors'}
foreach($name in @('Hex','Unhex','Parts','Logical','Desired','EqualAcl','Equal')){
 $f=$ast.Find({param($n) $n-is[System.Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-ceq$name},$true)
 if(-not$f){throw 'Helper missing'};Invoke-Expression $f.Extent.Text
}
$before=[byte[]]::new(52);$before[0]=1;[BitConverter]::GetBytes([UInt16]0xb004).CopyTo($before,2)
[BitConverter]::GetBytes([UInt32]20).CopyTo($before,4);[BitConverter]::GetBytes([UInt32]32).CopyTo($before,8);[BitConverter]::GetBytes([UInt32]44).CopyTo($before,16)
foreach($offset in @(20,32)){$before[$offset]=1;$before[$offset+1]=1;$before[$offset+7]=5};$before[28]=18;$before[40]=19;$before[44]=2;$before[46]=8
$logicalBefore=Logical $before
$inputData=Get-Content -LiteralPath $InputJson -Raw|ConvertFrom-Json
foreach($challenge in $inputData.challenges){$ace=Unhex $challenge.ace_hex;$desired=Desired $before $ace $challenge.acl_revision;$parsed=Logical $desired
 if($parsed.owner-cne$logicalBefore.owner-or$parsed.group-cne$logicalBefore.group-or-not(EqualAcl $parsed.dacl $logicalBefore.dacl)){throw 'Scratch own SID/DACL overwritten'}
 if($parsed.sacl.aces.Count-ne1-or$parsed.sacl.aces[0].raw-cne$challenge.ace_hex){throw 'Copied ACE changed'}
 if(($parsed.control-band0x3000)-ne($logicalBefore.control-band0x3000)){throw 'Protection bits changed'}
}
# Repacked owner/group buffers represent the same logical descriptor.
$repacked=[byte[]]$before.Clone();[Array]::Copy($before,20,$repacked,32,12);[Array]::Copy($before,32,$repacked,20,12)
[BitConverter]::GetBytes([UInt32]32).CopyTo($repacked,4);[BitConverter]::GetBytes([UInt32]20).CopyTo($repacked,8)
if((Hex $before)-ceq(Hex $repacked)-or-not(Equal (Logical $before) (Logical $repacked))){throw 'Packing misclassified as logical failure'}
$padding=[byte[]]::new(56);[Array]::Copy($before,$padding,52);$padding[46]=12
if(-not(Equal (Logical $before) (Logical $padding))){throw 'ACL unused padding misclassified'}
$changed=[byte[]]$before.Clone();$changed[28]=20
if(Equal (Logical $before) (Logical $changed)){throw 'Logical owner change ignored'}
$malformed=[byte[]]$before.Clone();$malformed[4]=19;$rejected=$false;try{Logical $malformed|Out-Null}catch{$rejected=$true};if(-not$rejected){throw 'Malformed offset accepted'}
'Seven actual opaque source ACE20 variants preserved; own SID/DACL/protection maintained; raw packing/padding distinguished; malformed bounds rejected'

$source=(Get-Content -LiteralPath $path -Raw).Split("Add-Type -TypeDefinition @'`n")[1].Split("`n'@")[0];Add-Type -TypeDefinition $source
if([ReservedTrustScratch]::Protection([UInt16]0xb004)-ne[UInt32]3221225472){throw "Protected DACL/SACL composition wrong"}
