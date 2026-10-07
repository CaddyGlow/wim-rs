$ErrorActionPreference='Stop'
# PowerShell unwraps a one-row function result into a dictionary. Wrap each
# function result before concatenation so identical keys cannot merge.
function Entries([string]$parent){[ordered]@{Parent=$parent;LongName='OnlyEntry';Alias='ONLY~1'}}
$same='U:\same';$other='U:\other'
$rows=(@(Entries $same)+@(Entries $other))
if($rows.Count -ne 2 -or $rows[0].Parent -ne $same -or $rows[1].Parent -ne $other){throw 'Single-entry parent arrays were not preserved'}
$source=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'windows-dos-alias-creation-scratch.ps1') -Raw
if($source.Contains('@((Entries $same)+(Entries $other))')){throw 'Dictionary merge regression in harness'}
if(([regex]::Matches($source,[regex]::Escape('(@(Entries $same)+@(Entries $other))'))).Count -ne 3){throw 'Expected all three parent concatenations'}
Write-Output 'Single-entry parent regression passed: two independent rows; all three harness sites guarded'
