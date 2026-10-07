#!/usr/bin/env python3
"""Boot independently verified Rust-reconstructed media in blank BIOS/UEFI guests."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import time

spec = importlib.util.spec_from_file_location('boot_helpers', Path(__file__).resolve().parents[3] / 'windows-uup/scripts/native-copy-boot-smoke.py')
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['start', 'capture', 'secure-state', 'focus-setup', 'stop'])
    parser.add_argument('--verification', type=Path, required=True)
    parser.add_argument('--state', type=Path, required=True)
    args = parser.parse_args()
    manifest = args.state / 'boot.json'
    if args.action == 'start':
        verification = json.loads(args.verification.read_text())
        iso = Path(verification['iso']).resolve(strict=True)
        if not verification.get('transfer_hash_matches') or helpers.sha(iso) != verification['iso_sha256']:
            raise RuntimeError('Requires the exact independently verified and hash-matched ISO')
        if any(step['exit'] != 0 for step in verification['steps']):
            raise RuntimeError('A media verification gate failed')
        args.state.mkdir(mode=0o700, parents=True, exist_ok=False)
        evidence = {'iso': str(iso), 'iso_sha256': verification['iso_sha256'],
                    'verification': str(args.verification), 'cases': {},
                    'scope': 'Setup boot only; no installed-target verification'}
        for case in ['bios', 'uefi', 'secure']:
            state = (args.state / case).resolve()
            state.mkdir(mode=0o700)
            disk = state / 'disk.qcow2'
            subprocess.run(['qemu-img', 'create', '-f', 'qcow2', str(disk), '32G'], check=True, capture_output=True)
            firmware = []
            if case != 'bios':
                code = helpers.DEFAULT_FIRMWARE / 'OVMF_CODE.fd'
                original = helpers.DEFAULT_FIRMWARE / 'OVMF_VARS.fd'
                variables = state / 'OVMF_VARS.fd'
                if case == 'secure':
                    subprocess.run([helpers.DEFAULT_VARS_TOOL, '--input', str(original), '--output', str(variables),
                                    '--enroll-redhat', '--microsoft-db', 'all', '--microsoft-kek', 'all', '--secure-boot'],
                                   check=True, capture_output=True)
                else:
                    shutil.copyfile(original, variables)
                firmware = ['-drive', f'if=pflash,format=raw,unit=0,readonly=on,file={code}',
                            '-drive', f'if=pflash,format=raw,unit=1,file={variables}']
            command = ['qemu-system-x86_64', '-name', 'rust-uup-' + case, '-enable-kvm',
                       '-machine', 'pc' if case == 'bios' else 'q35,smm=on',
                       '-cpu', 'host', '-m', '4096', '-smp', '2', *firmware]
            if case == 'bios':
                command += ['-drive', f'file={disk},format=qcow2,if=ide,index=0',
                            '-drive', f'file={iso},format=raw,media=cdrom,readonly=on,if=ide,index=2']
            else:
                command += ['-device', 'ich9-ahci,id=sata',
                            '-drive', f'if=none,id=disk,file={disk},format=qcow2',
                            '-device', 'ide-hd,bus=sata.0,drive=disk',
                            '-drive', f'if=none,id=iso,file={iso},format=raw,media=cdrom,readonly=on',
                            '-device', 'ide-cd,bus=sata.2,drive=iso,bootindex=0']
            command += ['-boot', 'order=d,menu=off', '-nic', 'none', '-display', 'none',
                        '-serial', f'file:{state}/serial.log', '-monitor', 'none',
                        '-qmp', f'unix:{state}/qmp.sock,server=on,wait=off']
            with (state / 'qemu.stdout').open('wb') as stdout, (state / 'qemu.stderr').open('wb') as stderr:
                process = subprocess.Popen(command, stdout=stdout, stderr=stderr, start_new_session=True)
            evidence['cases'][case] = {'pid': process.pid, 'command': command, 'visual_result': 'pending'}
            manifest.write_text(json.dumps(evidence, indent=2) + '\n')
            deadline = time.monotonic() + 15
            while not (state / 'qmp.sock').exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError((state / 'qemu.stderr').read_text())
                time.sleep(.1)
            qmp = helpers.Qmp(state / 'qmp.sock')
            try:
                for _ in range(40):
                    qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': 'spc'}], 'hold-time': 30})
                    time.sleep(.25)
            finally:
                qmp.close()
        print(str(manifest), flush=True)
        return
    evidence = json.loads(manifest.read_text())
    if args.action == 'focus-setup':
        state = (args.state / 'secure').resolve()
        qmp = helpers.Qmp(state / 'qmp.sock')
        try:
            qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': 'alt'},
                                            {'type': 'qcode', 'data': 'tab'}], 'hold-time': 100})
            time.sleep(2)
            ppm = state / 'screen.ppm'
            qmp.request('screendump', {'filename': str(ppm)})
            helpers.png_from_ppm(ppm)
        finally:
            qmp.close()
        return
    if args.action == 'secure-state':
        state = (args.state / 'secure').resolve()
        qmp = helpers.Qmp(state / 'qmp.sock')
        try:
            qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': 'shift'},
                                            {'type': 'qcode', 'data': 'f10'}], 'hold-time': 100})
            time.sleep(2)
            commands = ['wpeutil updatebootinfo',
                        r'reg query hklm\system\currentcontrolset\control\secureboot\state']
            for command in commands:
                for character in command:
                    code = {' ': 'spc', '\\': 'backslash'}.get(character, character)
                    qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': code}], 'hold-time': 30})
                    time.sleep(.04)
                qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': 'ret'}], 'hold-time': 50})
                time.sleep(2)
            ppm = state / 'secure-state.ppm'
            qmp.request('screendump', {'filename': str(ppm)})
            helpers.png_from_ppm(ppm)
            for code in ['e', 'x', 'i', 't', 'ret']:
                qmp.request('send-key', {'keys': [{'type': 'qcode', 'data': code}], 'hold-time': 30})
                time.sleep(.05)
            time.sleep(1)
            ppm = state / 'screen.ppm'
            qmp.request('screendump', {'filename': str(ppm)})
            helpers.png_from_ppm(ppm)
            evidence['cases']['secure']['state_query_commands'] = commands
            manifest.write_text(json.dumps(evidence, indent=2) + '\n')
        finally:
            qmp.close()
        return
    for case in evidence['cases']:
        state = (args.state / case).resolve()
        qmp = helpers.Qmp(state / 'qmp.sock')
        try:
            if args.action == 'stop':
                qmp.request('quit')
            else:
                ppm = state / 'screen.ppm'
                qmp.request('screendump', {'filename': str(ppm)})
                helpers.png_from_ppm(ppm)
                (state / 'qmp.json').write_text(json.dumps({
                    'status': qmp.request('query-status'), 'block': qmp.request('query-block')}, indent=2) + '\n')
        finally:
            qmp.close()
    print(str(manifest), flush=True)


if __name__ == '__main__':
    main()
