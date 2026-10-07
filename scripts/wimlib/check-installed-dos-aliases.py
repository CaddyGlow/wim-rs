#!/usr/bin/env python3
"""Bounded, read-only Windows long/DOS-name comparison after real OOBE."""
import argparse
import hashlib
import json
from pathlib import Path
import re
from windows_guest import WindowsGuest


def digest(data):
    return hashlib.sha256(data).hexdigest()


def validate_report(expected, report, inventory_hash):
    if report['InventorySHA256'].lower() != inventory_hash:
        raise ValueError('guest inventory differs from the bound selector batch')
    rows = report['Rows']
    if report['Count'] != len(expected) or len(rows) != len(expected):
        raise ValueError('guest omitted or duplicated selector rows')
    for source, result in zip(expected, rows, strict=True):
        if (result['PathUTF16'], result['AliasUTF16']) != (source['path_utf16_le'], source['alias_utf16_le']):
            raise ValueError('guest changed or reordered raw UTF-16 selectors')
        if result['Pass']:
            if (not isinstance(result['LongFileID'], str)
                    or re.fullmatch(r'[0-9A-F]{48}', result['LongFileID']) is None
                    or result['AliasFileID'] != result['LongFileID']):
                raise ValueError('successful result lacks matching complete FILE_ID_INFO')
    failures = sum(result['Pass'] is not True for result in rows)
    if report['Failures'] != failures:
        raise ValueError('guest failure count does not match actual rows')
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', type=Path, required=True)
    parser.add_argument('--inventory-sha256', required=True)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    original = args.inventory.read_bytes()
    if digest(original) != args.inventory_sha256.lower():
        raise ValueError('independent selector SHA256 mismatch')
    selectors = json.loads(original)
    if not isinstance(selectors, list) or not selectors:
        raise ValueError('expected nonempty raw UTF-16 selector list')
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {'scope': 'all bound source selectors, actual installed no-follow FILE_ID_INFO',
                'inventory': str(args.inventory.resolve()), 'inventory_sha256': digest(original),
                'count': len(selectors), 'actual_target_checked': False, 'batches': []}
    batches = []
    for start in range(0, len(selectors), 1000):
        selected = selectors[start:start + 1000]
        payload = (json.dumps(selected, separators=(',', ':'), ensure_ascii=True) + '\n').encode()
        name = f'batch-{start:06d}'
        (args.output / (name + '-selectors.json')).write_bytes(payload)
        row = {'start': start, 'count': len(selected), 'name': name,
               'selector_sha256': digest(payload)}
        manifest['batches'].append(row)
        batches.append((row, payload, selected))
    manifest_path = args.output / 'manifest.json'
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    if args.prepare_only:
        print(json.dumps({'prepared': len(batches), 'entries': len(selectors), 'actual_target_checked': False}))
        return
    state = json.loads((args.state / 'installation.json').read_text())
    user = state['interactive_user']
    if re.fullmatch(r'[A-Za-z0-9_.-]{1,20}', user) is None:
        raise ValueError('invalid recorded installation account')
    guest = WindowsGuest(Path(state['vm_state']) / 'qga.sock')
    ps = r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'
    readiness = guest.execute(ps, ['-NoProfile', '-NonInteractive', '-Command',
        r"$s=Get-ItemProperty 'HKLM:\SYSTEM\Setup';$i=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Setup\State').ImageState;"
        + r"$p=@(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue|Where-Object{$_.SessionId -gt 0 -and $_.UserName.EndsWith(([string][char]92+'INSTALLATION_ACCOUNT'),[StringComparison]::OrdinalIgnoreCase)});".replace('INSTALLATION_ACCOUNT', user)
        + r"if(-not(Test-Path C:\qcow2-install-gate-ready.txt)-or $p.Count -eq 0 -or $s.SystemSetupInProgress -ne 0 -or $s.OOBEInProgress -ne 0 -or $i -ne 'IMAGE_STATE_COMPLETE'){exit 1};New-Item C:\qcow2-alias-gate -ItemType Directory -Force|Out-Null"], timeout=60)
    if readiness['exit'] != 0:
        raise RuntimeError('requires actual completed OOBE, ready marker and expected Explorer session')
    script = Path(__file__).with_name('windows-distinct-dos-aliases.ps1').read_bytes()
    guest.put(r'C:\qcow2-alias-gate\check.ps1', script)
    manifest['script_sha256'] = digest(script)
    total_failures = 0
    for row, payload, selected in batches:
        prefix = 'C:\\qcow2-alias-gate\\' + row['name']
        guest.put(prefix + '-selectors.json', payload)
        run = guest.execute(ps, ['-NoProfile', '-NonInteractive', '-File',
                                 r'C:\qcow2-alias-gate\check.ps1', '-Inventory', prefix + '-selectors.json',
                                 '-Output', prefix + '-result.json'], timeout=300)
        (args.output / (row['name'] + '-execute.json')).write_text(json.dumps(run, indent=2) + '\n')
        try:
            output = guest.get(prefix + '-result.json')
            (args.output / (row['name'] + '-result.json')).write_bytes(output)
        except Exception as error:
            row['result_retrieval_error'] = str(error)
            manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
            raise RuntimeError('alias batch result unavailable; raw execute result retained') from error
        if run['exit'] != 0 or run.get('stdout_truncated'):
            raise RuntimeError('alias batch did not complete successfully')
        receipt = json.loads(run['stdout'])
        if digest(output) != receipt['OutputSHA256'].lower():
            raise ValueError('result file differs from guest receipt')
        report = json.loads(output)
        failures = validate_report(selected, report, row['selector_sha256'])
        (args.output / (row['name'] + '-result.json')).write_bytes(output)
        row.update(result_sha256=digest(output), failures=failures, completed=True)
        total_failures += failures
        manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    manifest.update(actual_target_checked=True, failures=total_failures, passes=total_failures == 0)
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({'entries': len(selectors), 'failures': total_failures, 'passes': manifest['passes']}))
    if total_failures:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
