#!/usr/bin/env python3
"""Record exact Windows CRT output bytes from original-header print functions."""
import argparse
import hashlib
import json
import pathlib
from windows_guest import WindowsGuest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qga-socket', required=True)
    parser.add_argument('--dll', type=pathlib.Path, default=pathlib.Path('/tmp/wimlib-windows-oracle/.libs/libwim-15.dll'))
    parser.add_argument('--probe', type=pathlib.Path, default=pathlib.Path('target/windows-abi-probe/probe-windows-print.exe'))
    parser.add_argument('--implementation', default='original')
    parser.add_argument('--baseline', type=pathlib.Path)
    parser.add_argument('--expanded', action='store_true')
    parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('docs/wimlib/evidence/native-windows-print/original.json'))
    args = parser.parse_args()
    guest = WindowsGuest(args.qga_socket)
    root = r'C:\wim-print-20261003' + '\\' + args.implementation
    assert guest.powershell("New-Item -ItemType Directory -Force '" + root + "' | Out-Null")['exit'] == 0
    artifacts = {}
    for label, host, name in [('dll', args.dll, 'wim.dll'), ('caller', args.probe, 'probe.exe'),
                              ('fixture', pathlib.Path('crates/wim-format/tests/fixtures/xpress-resource.wim'), 'fixture-é漢.wim')]:
        data = host.read_bytes()
        guest.put(root + '\\' + name, data)
        artifacts[label] = {'sha256': hashlib.sha256(data).hexdigest(), 'host_path': str(host), 'guest_path': root + '\\' + name}
    observations = []
    cases = [(mode, None, None, None) for mode in ['text', 'binary']]
    if args.expanded:
        cases += [(mode, image, variant, locale) for mode in ['text', 'binary']
                  for image in [-2, -1, 0, 1, 2] for variant in ['newline', 'unpaired']
                  for locale in ['C', 'french']]
    for mode, image, variant, locale in cases:
        arguments = [artifacts['dll']['guest_path'], artifacts['fixture']['guest_path'], mode]
        if image is not None:
            arguments += [str(image), variant, locale]
        result = guest.execute(artifacts['caller']['guest_path'], arguments)
        observations.append({'mode': mode, 'image': image, 'variant': variant, 'locale': locale, 'result': result})
    result = {'scope': 'Actual Windows print header/all-images, unchanged-header MinGW/MSVCRT caller and exact raw output bytes',
              'implementation': args.implementation, 'artifacts': artifacts, 'cases': observations}
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        result['differences'] = [{'mode': a['mode'], 'original': a['result'], 'native': b['result']}
                                 for a, b in zip(baseline['cases'], observations) if a['result'] != b['result']]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'cases': len(observations), 'exits': [entry['result']['exit'] for entry in observations],
                      'differences': len(result.get('differences', []))}, indent=2))


if __name__ == '__main__':
    main()
