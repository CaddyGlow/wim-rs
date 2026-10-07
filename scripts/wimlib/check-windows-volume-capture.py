#!/usr/bin/env python3
"""Capture a frozen Windows directory, verify upstream, and optionally apply with DISM."""
import argparse
import hashlib
import json
from pathlib import Path
import time

from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--fixture-evidence', required=True, type=Path)
    parser.add_argument('--dll', required=True, type=Path)
    parser.add_argument('--probe', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apply', action='store_true', help='apply into a fresh disposable guest directory')
    args = parser.parse_args()
    fixture = json.loads(args.fixture_evidence.read_text())
    guest = WindowsGuest(args.qga_socket)
    root = fixture['fixture'] + '\\volume-' + str(time.time_ns())
    source = fixture['snapshot']['DeviceObject'] + r'\Windows'
    evidence = {'fixture': root, 'source': source, 'snapshot': fixture['snapshot'],
                'environment': fixture['setup'], 'artifacts': {
                    'original.dll': fixture['artifacts']['original.dll']}}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        args.output.write_text(json.dumps(evidence, indent=2) + '\n')
    result = guest.powershell("$ErrorActionPreference='Stop';New-Item -ItemType Directory '" + root + "'|Out-Null")
    if result['exit'] != 0:
        raise RuntimeError(result)
    for name, path in [('rust.dll', args.dll), ('volume.exe', args.probe)]:
        data = path.read_bytes()
        guest.put(root + '\\' + name, data)
        evidence['artifacts'][name] = hashlib.sha256(data).hexdigest()
    original = guest.get(fixture['fixture'] + r'\original.dll')
    if hashlib.sha256(original).hexdigest() != evidence['artifacts']['original.dll']:
        raise RuntimeError('original DLL hash differs from fixture evidence')
    guest.put(root + r'\original.dll', original)
    output = root + r'\windows.wim'
    started = time.monotonic()
    save()
    result = guest.execute(root + r'\volume.exe', [root + r'\rust.dll', source, output,
                           root + r'\original.dll'], timeout=3600)
    result.pop('stdout_base64', None)
    evidence.update({'result': result, 'elapsed_seconds': round(time.monotonic() - started, 2)})
    save()
    passes = (result['exit'] == 0 and not result.get('stdout_truncated') and
              all(marker in result['stdout'].splitlines() for marker in
                  ['init 0', 'add 0', 'write 0', 'independent-open 0', 'independent-verify 0']) and
              'independent-tree 0 entries=' in result['stdout'])
    if passes and args.apply:
        target = root + r'\windows-dism'
        started = time.monotonic()
        command = ("$ErrorActionPreference='Stop';New-Item -ItemType Directory '" + target +
                   "'|Out-Null;& dism.exe /Apply-Image '/ImageFile:" + output +
                   "' /Index:1 '/ApplyDir:" + target + "' /CheckIntegrity /EA '/LogPath:" + root +
                   "\\dism.log';exit $LASTEXITCODE")
        result = guest.execute(r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',
                               ['-NoProfile', '-NonInteractive', '-Command', command], timeout=3600)
        result.pop('stdout_base64', None)
        evidence.update({'dism_apply': result, 'apply_elapsed_seconds': round(time.monotonic() - started, 2)})
        passes &= result['exit'] == 0 and not result.get('stdout_truncated')
        save()
    if passes:
        result = guest.powershell("$ErrorActionPreference='Stop';$p='" + output +
                                  "';[PSCustomObject]@{Size=(Get-Item -LiteralPath $p).Length;SHA256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash}|ConvertTo-Json")
        result.pop('stdout_base64', None)
        evidence['archive_inventory'] = result
        passes &= result['exit'] == 0 and not result.get('stdout_truncated')
    evidence['passes'] = passes
    save()
    print(json.dumps({'passes': passes, 'applied': args.apply}), flush=True)
    return 0 if passes else 1


if __name__ == '__main__':
    raise SystemExit(main())
