#!/usr/bin/env python3
"""Run real UUP reconstruction and an audited servicing gate in an owned VM."""
import argparse
import hashlib
import json
from pathlib import Path
import time

from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--servicing-harness', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source', default=r'D:\UUPs')
    parser.add_argument('--oracle', default=r'D:\tools\wimlib-imagex.exe')
    parser.add_argument('--timeout', type=int, default=10800)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-uup-validation-' + str(time.time_ns())
    evidence = {'guest_root': root, 'source': args.source, 'artifacts': {}, 'steps': []}

    def save():
        (args.output / 'run.json').write_text(json.dumps(evidence, indent=2) + '\n')

    def step(name, executable, arguments):
        started = time.monotonic()
        result = guest.execute(executable, arguments, timeout=args.timeout)
        result.pop('stdout_base64', None)
        result.update(name=name, executable=executable, arguments=arguments,
                      elapsed_seconds=time.monotonic() - started)
        evidence['steps'].append(result)
        save()
        print(json.dumps({'step': name, 'exit': result['exit'],
                          'elapsed_seconds': result['elapsed_seconds']}), flush=True)
        return result

    def collect_job(remote, label):
        for filename in ['journal.jsonl', 'state.json']:
            (args.output / (label + '-' + filename)).write_bytes(guest.get(remote + '\\' + filename))
        listing = guest.powershell(
            "@(Get-ChildItem -LiteralPath 'REMOTE\\logs' -File | Select-Object -ExpandProperty Name) | ConvertTo-Json -Compress"
            .replace('REMOTE', remote))
        if listing['exit'] != 0:
            raise RuntimeError(listing)
        names = json.loads(listing['stdout'] or '[]') or []
        if isinstance(names, str):
            names = [names]
        destination = args.output / (label + '-logs')
        destination.mkdir()
        for name in names:
            if Path(name).name != name or '\\' in name:
                raise RuntimeError('Unsafe guest log filename')
            try:
                (destination / name).write_bytes(guest.get(remote + '\\logs\\' + name))
            except RuntimeError as error:
                # A failed servicing job can leave a logger holding its file.
                # Retain the command failure and continue collecting the other
                # evidence instead of masking it with a transfer failure.
                evidence.setdefault('log_collection_errors', []).append({
                    'job': remote, 'file': name, 'error': str(error)})
                save()

    preflight = guest.powershell(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Directory -Path 'ROOT' | Out-Null; "
        "[ordered]@{os=(Get-CimInstance Win32_OperatingSystem).Caption; "
        "account=(& whoami); source=(Get-Item 'SOURCE').FullName; "
        "volume=(Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='D:'\" | "
        "Select-Object DriveType,FileSystem); "
        "payloads=@(Get-ChildItem 'SOURCE' -File | Sort-Object Name | "
        "ForEach-Object {[ordered]@{name=$_.Name;size=$_.Length;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash}})} "
        "| ConvertTo-Json -Depth 6"
        .replace('ROOT', root).replace('SOURCE', args.source))
    preflight.pop('stdout_base64', None)
    evidence['preflight'] = preflight
    save()
    if preflight['exit'] != 0:
        raise RuntimeError(preflight)
    source_before = json.loads(preflight['stdout'])
    if source_before['volume']['DriveType'] != 5:
        raise RuntimeError('Source must be on the read-only optical fixture')
    for name, path in [('windows-uup.exe', args.cli), ('servicing.exe', args.servicing_harness)]:
        data = path.read_bytes()
        evidence['artifacts'][name] = hashlib.sha256(data).hexdigest()
        guest.put(root + '\\' + name, data)
    save()
    cli = root + r'\windows-uup.exe'
    reconstruction = step('reconstruct-base', cli, [
        'reconstruct-base', '--uup-dir', args.source, '--metadata',
        args.source + r'\professional_en-us.esd', '--work-dir', root + r'\assembly'])
    assembly = root + r'\assembly'
    collect_job(assembly, 'assembly')
    if reconstruction['exit'] != 0:
        return 1
    for image in ['install', 'boot']:
        path = assembly + '\\base-media\\sources\\' + image + '.wim'
        if step('oracle-verify-' + image, args.oracle, ['verify', path])['exit'] != 0:
            return 1
        if step('oracle-info-' + image, args.oracle, ['info', path])['exit'] != 0:
            return 1
    package = next(p for p in source_before['payloads'] if p['name'] == 'SSU-19041.7714-x64.cab')
    servicing = step('servicing', root + r'\servicing.exe', [
        root + r'\servicing', assembly + r'\base-media\sources\install.wim',
        args.source + '\\' + package['name'],
        'Package_for_ServicingStack_7714~31bf3856ad364e35~amd64~~19041.7714.1.3',
        package['sha256']])
    collect_job(root + '\\servicing', 'servicing')
    postflight = guest.powershell(
        "$ErrorActionPreference='Stop'; [ordered]@{payloads=@(Get-ChildItem 'SOURCE' -File | Sort-Object Name | "
        "ForEach-Object {[ordered]@{name=$_.Name;size=$_.Length;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash}}); "
        "mounts=(& dism.exe /English /Get-MountedWimInfo); "
        "images=@(Get-ChildItem 'ROOT\\assembly' -Recurse -Filter *.wim | "
        "ForEach-Object {[ordered]@{path=$_.FullName;size=$_.Length;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash}})} "
        "| ConvertTo-Json -Depth 6".replace('SOURCE', args.source).replace('ROOT', root))
    postflight.pop('stdout_base64', None)
    evidence['postflight'] = postflight
    evidence['source_payloads_preserved'] = (
        postflight['exit'] == 0 and json.loads(postflight['stdout'])['payloads'] == source_before['payloads'])
    evidence['scope'] = 'Real UUP base reconstruction and one audited SSU; no installation or requested final target claim'
    save()
    return 0 if servicing['exit'] == 0 and evidence['source_payloads_preserved'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
