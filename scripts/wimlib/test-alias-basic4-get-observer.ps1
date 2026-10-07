# Host-only path guards; no Windows queries, creators or setters.
$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot 'windows-alias-basic4-get-observer.ps1'
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Observer syntax errors' }
$decode = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Decode'
}, $true)
if (-not $decode) { throw 'Observer Decode function missing' }
Invoke-Expression $decode.Extent.Text
$compare = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'CompareExpected'
}, $true)
if (-not $compare) { throw 'Observer comparison function missing' }
Invoke-Expression $compare.Extent.Text
$expected = '{"basic36":"abcd","file_id_info24":"1234","security_raw":"5678","object_id_full64":"9abc"}' | ConvertFrom-Json
$actual = @{ basic36 = 'abcd'; after_get_basic36 = 'abcd'; file_id_info24 = '1234'; security_raw = '5678'; object_id_full64 = '9abc'; volume_serial = 123; object_status = 0 }
if (-not (CompareExpected $actual $expected 123)) { throw 'JSON property comparison failed' }
foreach ($field in @('basic36', 'file_id_info24', 'security_raw', 'object_id_full64')) {
    $changed = $actual.Clone()
    $changed[$field] = 'different'
    if (CompareExpected $changed $expected 123) { throw 'Changed metadata accepted' }
    $changed.Remove($field)
    if (CompareExpected $changed $expected 123) { throw 'Missing metadata accepted' }
}
$changed = $actual.Clone()
$changed.after_get_basic36 = 'different'
if (CompareExpected $changed $expected 123) { throw 'GET changed basic metadata' }
if (CompareExpected $actual $expected 124) { throw 'Wrong volume accepted' }
$expected.object_id_full64 = ''
$actual.object_id_full64 = ''
foreach ($status in @(2, 4312)) {
    $actual.object_status = $status
    if (-not (CompareExpected $actual $expected 123)) { throw 'Bound empty ObjectID absence rejected' }
}
foreach ($status in @(0, 5, 13)) {
    $actual.object_status = $status
    if (CompareExpected $actual $expected 123) { throw 'Unexpected ObjectID status accepted' }
}
$x = @{ scratch_root = 'D:\alias-basic4-cycle-0123456789abcdef0123456789abcdef' }
function Raw([string]$value) {
    [BitConverter]::ToString([Text.Encoding]::Unicode.GetBytes($value)).Replace('-', '').ToLowerInvariant()
}
foreach ($leaf in @('file.bin', 'trailing space ', 'trailing dot.')) {
    $valid = $x.scratch_root + '\' + $leaf
    if ((Decode (Raw $valid)) -cne ('\\?\' + $valid)) { throw 'Extended path changed' }
}
foreach ($invalid in @(
    ($x.scratch_root + '\..\escape'),
    ($x.scratch_root + '\.\file'),
    ($x.scratch_root + '\file:stream'),
    ($x.scratch_root + '\\file'),
    'C:\other\file',
    ($x.scratch_root + '/file'),
    ($x.scratch_root + '\nul' + [char]0)
)) {
    $rejected = $false
    try { Decode (Raw $invalid) | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw 'Selector escapes accepted' }
}
'Observer extended paths remain literal; traversal, ADS, drive, separator and NUL escapes fail closed'

if((Decode (Raw $x.scratch_root))-cne("\\?\"+$x.scratch_root)){throw "Exact bound parent root rejected"}
$rejected=$false;try{Decode (Raw ($x.scratch_root+"-other"))|Out-Null}catch{$rejected=$true};if(-not$rejected){throw "Root prefix collision accepted"}
$source=(Get-Content -LiteralPath $path -Raw).Split("Add-Type -TypeDefinition @'`n")[1].Split("`n'@")[0];Add-Type -TypeDefinition $source

if($source.Contains('SetFileShortName')-or$source.Contains('NtSetInformation')-or$source.Contains('CreateHardLink')-or$source.Contains('0x900c0')){throw 'Unexpected creator/setter in observer'}
