#!/usr/bin/env python3
"""Original/native real reparse capture on one disposable owned Windows fixture."""
import argparse
import hashlib
import json
from pathlib import Path
from windows_guest import WindowsGuest
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--qga-socket', required=True)
p.add_argument('--dll', type=Path, required=True)
p.add_argument('--probe', type=Path, default=Path('/tmp/probe-windows-capture-reparse.exe'))
p.add_argument('--label', required=True)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
g = WindowsGuest(a.qga_socket)
root = r'C:\wim-capture-reparse-20261003'
setup = g.powershell(r'''
$ErrorActionPreference='Stop'; $root='C:\wim-capture-reparse-20261003';
New-Item -ItemType Directory -Force $root | Out-Null;
$source=$root+'\source';
if(-not(Test-Path -LiteralPath $source)) {
 New-Item -ItemType Directory $source | Out-Null;
 New-Item -ItemType Directory ($source+'\directory') | Out-Null;
 [IO.File]::WriteAllBytes($source+'\file-é漢.bin',[Text.Encoding]::ASCII.GetBytes('reparse-real-payload'));
 [IO.File]::WriteAllBytes($source+'\directory\child',[byte[]](1,2,3));
 [IO.File]::WriteAllBytes($root+'\outside',[byte[]](4,5,6));
 cmd.exe /c ('mklink "'+$source+'\relative" "file-é漢.bin"'); if($LASTEXITCODE){throw 'relative symlink failed'};
 cmd.exe /c ('mklink "'+$source+'\absolute" "'+$source+'\file-é漢.bin"'); if($LASTEXITCODE){throw 'absolute symlink failed'};
 cmd.exe /c ('mklink /D "'+$source+'\dirlink" "'+$source+'\directory"'); if($LASTEXITCODE){throw 'directory symlink failed'};
 cmd.exe /c ('mklink /J "'+$source+'\junction" "'+$source+'\directory"'); if($LASTEXITCODE){throw 'junction failed'};
 cmd.exe /c ('mklink "'+$source+'\external" "'+$root+'\outside"'); if($LASTEXITCODE){throw 'external symlink failed'};
 cmd.exe /c ('mklink "'+$source+'\dangling" "'+$source+'\missing"'); if($LASTEXITCODE){throw 'dangling symlink failed'};
 cmd.exe /c ('mklink /D "'+$root+'\rootlink" "'+$source+'"'); if($LASTEXITCODE){throw 'root symlink failed'};
}
Get-ChildItem -LiteralPath $source -Force | Select-Object Name,Attributes,LinkType,Target | ConvertTo-Json -Compress
''')
if setup['exit'] != 0: raise RuntimeError(setup)
run = root + '\\' + a.label
assert g.powershell("New-Item -ItemType Directory -Force '" + run + "' | Out-Null")['exit'] == 0
g.put(run + r'\wim.dll', a.dll.read_bytes())
g.put(run + r'\probe.exe', a.probe.read_bytes())
results = []
for name, flags, source, stop, value, init in [
    ('default', 0, 'source', 0, 0, None), ('no-acls', 0x20, 'source', 0, 0, None),
    ('rpfix-verbose', 0x1a4, 'source', 0, 0, None), ('norpfix-verbose', 0x2a4, 'source', 0, 0, None),
    ('rootlink', 0x1a4, 'rootlink', 0, 0, None), ('root-file-link', 0x1a4, r'source\absolute', 0, 0, None),
    ('cancel-fixup', 0x1a4, 'source', 10, 1, None), ('invalid-fixup-status', 0x1a4, 'source', 10, 91, None),
    ('cancel-begin', 0x1a4, 'source', 9, 1, None), ('cancel-end', 0x1a4, 'source', 11, 1, None),
    ('strict-acls', 0x40, 'source', 0, 0, None),
    ('dont-privileges', 0, 'source', 0, 0, 2),
    ('dont-strict-acls', 0x40, 'source', 0, 0, 2),
    ('dont-no-acls', 0x20, 'source', 0, 0, 2),
]:
    output = run + '\\' + name + '.wim'
    arguments = [run + r'\wim.dll', root + '\\' + source, output, str(flags), '-', str(stop), str(value)]
    if init is not None: arguments.append(str(init))
    r = g.execute(run + r'\probe.exe', arguments)
    if r['exit'] != 0: raise RuntimeError(r)
    results.append(dict(case=name, result=r))
    if 'write 0' in r['stdout']:
        out = a.output.parent / (a.label + '-' + name + '.wim')
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(g.get(output))
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(dict(dll_sha256=hashlib.sha256(a.dll.read_bytes()).hexdigest(),
    probe_sha256=hashlib.sha256(a.probe.read_bytes()).hexdigest(), setup=setup, results=results), indent=2)+'\n')
print(a.output)
