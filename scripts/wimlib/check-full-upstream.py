#!/usr/bin/env python3
"""Run preserved upstream portable scripts with the original ELF and frozen libraries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native', type=Path, default=Path('target/debug/libwim.so'))
    parser.add_argument('--output', type=Path, default=Path('docs/wimlib/evidence/native-full-upstream'))
    parser.add_argument('--timeout', type=int, default=1200)
    parser.add_argument('--strict-comparator', action='store_true', help='Make original capture/apply comparator failures fatal independently of /usr/bin/tree')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    source = Path('/tmp/wimlib')
    cli = Path('/tmp/wimlib-native-oracle/.libs/wimlib-imagex')
    original = Path('/tmp/wimlib-native-oracle/.libs/libwim.so').resolve()
    work = Path(tempfile.mkdtemp(prefix='wim-full-upstream-'))
    result = {'work': str(work), 'cli_sha256': sha(cli), 'native_sha256': sha(args.native),
              'original_sha256': sha(original), 'strict_comparator': args.strict_comparator, 'runs': []}
    scripts = ['test-imagex', 'test-imagex-capture_and_apply', 'test-imagex-update_and_extract']
    for kind, library in [('original', original), ('native', args.native)]:
        build = work / kind
        (build / 'tests').mkdir(parents=True)
        frozen = build / 'lib'
        frozen.mkdir()
        shutil.copy2(library, frozen / 'libwim.so')
        result[kind + '_sha256'] = sha(frozen / 'libwim.so')
        (frozen / 'libwim.so.15').symlink_to('libwim.so')
        shutil.copy2(cli, build / 'wimlib-imagex')
        subprocess.run(['cc', '-O2', '-I/tmp/wimlib-native-oracle', str(source / 'tests/tree-cmp.c'),
                        '-o', str(build / 'tests/tree-cmp')], check=True)
        env = os.environ | {'srcdir': str(source / 'tests'), 'LD_LIBRARY_PATH': str(frozen),
                            'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
        for script in scripts:
            shutil.copy2(source / 'tests' / script, build / 'tests' / script)
            if args.strict_comparator and script == 'test-imagex-capture_and_apply':
                path = build / 'tests' / script
                text = path.read_text()
                start = text.index('do_tree_cmp() {')
                end = text.index('\nimage_name=0', start)
                text = text[:start] + 'do_tree_cmp() {\n\t../tree-cmp in.dir out.dir || error \"Independent tree comparator failed\"\n}\n' + text[end:]
                path.write_text(text)
            log = args.output / f'{kind}-{script}.log'
            with log.open('wb') as stream:
                try:
                    completed = subprocess.run(['bash', str(build / 'tests' / script)], cwd=build,
                                               env=env, stdout=stream, stderr=subprocess.STDOUT,
                                               timeout=args.timeout)
                    status = completed.returncode
                except subprocess.TimeoutExpired:
                    status = 'timeout'
            result['runs'].append({'library': kind, 'script': script, 'script_sha256': sha(source / 'tests' / script),
                                   'executed_script_sha256': sha(build / 'tests' / script), 'status': status, 'log': str(log)})
            (args.output / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
            print(kind, script, status, flush=True)
            if status != 0:
                break
        if kind == 'original' and result['runs'][-1]['status'] != 0:
            print('Original baseline failed; native scripts were not run.', flush=True)
            break


if __name__ == '__main__':
    main()
