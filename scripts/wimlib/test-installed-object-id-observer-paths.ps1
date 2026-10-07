# Host-only test: evaluate just the selector function, never the observer/API calls.
$ErrorActionPreference='Stop'
$path=Join-Path $PSScriptRoot 'windows-observe-installed-object-ids.ps1'
$errors=$null;$tokens=$null
$ast=[System.Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Observer syntax errors'}
$function=$ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Decode'},$true)
if(-not $function){throw 'Decode missing'}
Invoke-Expression $function.Extent.Text
function Raw([string]$value){$hex='';foreach($c in $value.ToCharArray()){$n=[int]$c;$hex+=('{0:x2}{1:x2}' -f ($n -band 255),($n -shr 8))};return $hex}
foreach($relative in @('/dir/file.','/dir/file ',('/dir/'+[string][char]0xd800))){
 $expected='\\?\C:'+ $relative.Replace('/','\')
 $actual=Decode (Raw $relative)
 if($actual -cne $expected){throw 'Extended-length exact UTF16 selector changed'}
 if(-not $actual.StartsWith('\\?\C:\',[StringComparison]::Ordinal)){throw 'Extended-length prefix missing'}
}
foreach($relative in @('/../x','/./x','/dir//x','/dir\x','/dir:x','C:/x')){
 $failed=$false;try{Decode (Raw $relative)|Out-Null}catch{$failed=$true}
 if(-not $failed){throw 'Escaping selector accepted'}
}
'Extended-length/trailing-dot/trailing-space/unpaired-UTF16 selector regressions pass'

$logical=$ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'LogicalBasic'},$true)
$hexFunction=$ast.Find({param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Hex'},$true)
Invoke-Expression $hexFunction.Extent.Text
Invoke-Expression $logical.Extent.Text
$left=[byte[]]::new(40);$right=[byte[]]::new(40)
for($i=36;$i -lt 40;$i++){$right[$i]=255}
if((LogicalBasic $left) -cne (LogicalBasic $right)){throw 'Padding incorrectly treated as metadata'}
foreach($offset in @(0,8,16,24,32)){
 $changed=[byte[]]$left.Clone();$changed[$offset]=1
 if((LogicalBasic $left) -ceq (LogicalBasic $changed)){throw 'Logical timestamp/attribute change ignored'}
}
'FILE_BASIC_INFO padding excluded; all four timestamps/attributes checked'
