#!/usr/bin/env python3
"""Run unchanged-header extraction through real Windows anonymous pipes."""
import argparse
import hashlib
import json
import pathlib
import time
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-extract/pipe-original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-pipe-extract-20261003'
    run = root + '\\' + args.implementation + '-' + str(time.time_ns())
    assert guest.powershell("New-Item -ItemType Directory -Force '" + run + "' | Out-Null")['exit'] == 0
    dll = args.dll.read_bytes()
    original = pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll').read_bytes()
    caller = pathlib.Path('target/windows-abi-probe/probe-windows-extract-pipe.exe').read_bytes()
    guest.put(run + r'\wim.dll', dll)
    guest.put(run + r'\original.dll', original)
    guest.put(run + r'\probe.exe', caller)
    source = r'C:\wim-capture-20261003\native-default-write\capture-é漢.wim'
    source_bytes = guest.get(source)
    pipable = run + r'\actual-original-pipable.wim'
    generation = guest.execute(run + r'\probe.exe', [run + r'\original.dll', 'generate', source, pipable])
    assert generation['exit'] == 0 and 'write 0' in generation['stdout'], generation
    fixture = guest.get(pipable)
    cases = [(image, flags, chunk, stop, status, callback)
             for image, flags in [('1', 0), ('Capture-é漢𝄞', 0), ('2', 0), ('-null', 0), ('1', 64), ('1', 0x40000000)]
             for chunk in [7, 65536]
             for stop, status, callback in [(-1, 0, 1)]]
    cases += [('1', 0, 7, stop, status, 1) for stop in [0, 3, 4, 5, 6, 7] for status in [1, 2]]
    cases += [('1', 0, chunk, -1, 0, 0) for chunk in [7, 65536]]
    observations = []
    for index, (image, flags, chunk, stop, status, callback) in enumerate(cases):
        target = run + '\\target-' + str(index) + '-é漢'
        result = guest.execute(run + r'\probe.exe', [run + r'\wim.dll', 'extract', pipable, image, target,
                               str(flags), str(chunk), str(stop), str(status), str(callback)])
        assert not result['stdout_truncated']
        inventory = guest.powershell("$r='" + target + "';if(Test-Path -LiteralPath $r){@(Get-ChildItem -LiteralPath $r -Recurse -Force)|ForEach-Object{[PSCustomObject]@{Path=$_.FullName.Substring($r.Length);Directory=$_.PSIsContainer;Attrs=[int]$_.Attributes;Hash=$(if(-not $_.PSIsContainer){(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash})}}|ConvertTo-Json -Compress}else{Write-Output 'null'}")
        assert inventory['exit'] == 0, inventory
        observations.append({'image': image, 'flags': flags, 'chunk': chunk, 'stop': stop, 'status': status,
                             'callback': callback, 'result': result, 'inventory': json.loads(inventory['stdout'] or '[]')})
    assert guest.get(source) == source_bytes
    result = {'scope': 'Actual Windows anonymous pipe, matching MSVCRT fd, original-generated pipable input; drain only after API return',
              'implementation': args.implementation, 'dll_sha256': hashlib.sha256(dll).hexdigest(),
              'caller_sha256': hashlib.sha256(caller).hexdigest(), 'generation': generation,
              'input_path': pipable, 'input_size': len(fixture), 'input_sha256': hashlib.sha256(fixture).hexdigest(),
              'source_preserved': True, 'cases': observations}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        assert len(baseline['cases']) == len(observations)
        result['differences'] = [{'case': index, 'original': old, 'native': new}
                                 for index, (old, new) in enumerate(zip(baseline['cases'], observations))
                                 if old != new]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'source_preserved': True, 'input_size': len(fixture)}))


if __name__ == '__main__':
    main()
