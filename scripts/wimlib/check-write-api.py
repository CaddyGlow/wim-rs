#!/usr/bin/env python3
"""Compare native writer contracts and verify every output with original C."""
import hashlib
import os
import json
from pathlib import Path
import subprocess
import tempfile
from contextlib import nullcontext

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path('/tmp/wimlib-native-oracle')

def run(args):
    environment = os.environ.copy()
    if str(args[0]) == str(ORACLE / 'wimlib-imagex') or str(args[0]) == '/tmp/wim-write-original':
        environment['WIMLIB_DISABLE_CPU_FEATURES'] = 'sse4.2'
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True, env=environment)
    if result.returncode:
        raise RuntimeError(f'{globals().get("active_case")} {args}: {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout

for name, library in [('original', ORACLE / '.libs'), ('native', ROOT / 'target/debug')]:
    run(['cc', '-Wall', '-Wextra', '-Werror', '-I', '/tmp/wimlib/include', ROOT / 'scripts/wimlib/probe-write-api.c', '-L', library, f'-Wl,-rpath,{library}', '-lwim', '-o', f'/tmp/wim-write-{name}'])

cases = []
with nullcontext(tempfile.mkdtemp(prefix='wim-write-')) as directory:
    for source in ['new', '/tmp/metadata-native.wim']:
        before = None if source == 'new' else hashlib.sha256(Path(source).read_bytes()).hexdigest()
        for codec in range(4):
            for layout, flags in [('ordinary', 0), ('solid', 4096), ('pipable', 4)]:
                for mutation in ['none', 'append', 'delete']:
                    for fd in [0, 1]:
                        for selected_image in ([-1, 1, 2] if mutation == 'append' else [-1]):
                            active_case = (source, codec, layout, mutation, fd, selected_image)
                            paths = {name: Path(directory) / f'{name}.wim' for name in ['original', 'native']}
                            outputs = {name: run([f'/tmp/wim-write-{name}', source, path, mutation, codec, flags | 2048 | 1, selected_image, fd]) for name, path in paths.items()}
                            def contract(output):
                                return [line for line in output.splitlines() if line.startswith(('write ', 'written ', 'guid_retained ', 'verify '))]
                            assert contract(outputs['original']) == contract(outputs['native']), outputs
                            for path in paths.values():
                                run([ORACLE / 'wimlib-imagex', 'verify', path])
                            # A separate original extractor must consume the native output.
                            info = next(line for line in outputs['native'].splitlines() if line.startswith('written ')).split()
                            if int(info[1]):
                                target = Path(directory) / 'apply'
                                for image in range(1, int(info[1]) + 1):
                                    target.mkdir(exist_ok=True)
                                    run([ORACLE / 'wimlib-imagex', 'apply', paths['native'], str(image), target])
                                    import shutil
                                    shutil.rmtree(target)
                            cases.append({'source': source, 'codec': codec, 'layout': layout, 'mutation': mutation, 'fd': bool(fd), 'image': selected_image, 'contract_equal': True, 'original_verify': True, 'original_apply_images': int(info[1])})
        if before is not None:
            assert hashlib.sha256(Path(source).read_bytes()).hexdigest() == before
print(json.dumps({'cases': cases, 'count': len(cases), 'source_inputs_preserved': True, 'original_cli_disabled_cpu_feature': 'sse4.2', 'default_cpu_reader_crash': 'original-cli-crash.log'}, indent=2))
