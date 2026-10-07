#!/usr/bin/env python3
"""Run the genuine unchanged optional-test caller with frozen libraries."""
import argparse, hashlib, json, os, pathlib, re, shutil, subprocess, sys, tempfile
p = argparse.ArgumentParser()
p.add_argument('--seconds', type=int, default=60)
p.add_argument('--timeout', type=int)
p.add_argument('--output-prefix', required=True)
p.add_argument('--native', default='target/debug/libwim.so')
a = p.parse_args()
work = pathlib.Path(tempfile.mkdtemp(prefix='wim-genuine-wlfuzz-'))
prefix = pathlib.Path(a.output_prefix)
prefix.parent.mkdir(parents=True, exist_ok=True)
rows = {}
if a.seconds <= 0 or (a.timeout is not None and a.timeout <= 0):
    p.error('seconds and timeout must be positive')
for label, source in [('original', pathlib.Path('/tmp/wimlib-native-oracle/.libs/libwim.so')), ('native', pathlib.Path(a.native).resolve())]:
    d = work / label
    d.mkdir()
    lib = d / 'library'
    lib.mkdir()
    shutil.copy2(source, lib / 'libwim.so')
    (lib / 'libwim.so.15').symlink_to('libwim.so')
    sha = hashlib.sha256((lib / 'libwim.so').read_bytes()).hexdigest()
    cmd = ['cc', '-DHAVE_CONFIG_H', '-I/tmp/wimlib-native-oracle', '-I/tmp/wimlib/include', '/tmp/wimlib/tests/wlfuzz.c', '-L' + str(lib), '-Wl,-rpath,' + str(lib), '-lwim', '-o', str(d / 'wlfuzz')]
    c = subprocess.run(cmd, capture_output=True)
    row = dict(sha256=sha, compile_command=cmd, compile_status=c.returncode)
    pathlib.Path(str(prefix) + '-' + label + '-link.log').write_bytes(c.stdout + c.stderr)
    if c.returncode == 0:
        env = {**os.environ, 'LD_LIBRARY_PATH': str(lib), 'TMPDIR': str(d), 'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'}
        with pathlib.Path(str(prefix) + '-' + label + '.log').open('wb') as output:
            run_command = ['stdbuf', '-oL', '-eL', str(d / 'wlfuzz'), str(a.seconds)]
            row['run_command'] = run_command
            row['timeout_seconds'] = a.timeout or max(600, a.seconds + 600)
            try:
                r = subprocess.run(run_command, cwd=d, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=row['timeout_seconds'])
                row['status'] = r.returncode
            except subprocess.TimeoutExpired:
                row['timeout'] = True
        row['caller_sha256'] = hashlib.sha256((d / 'wlfuzz').read_bytes()).hexdigest()
        log = pathlib.Path(str(prefix) + '-' + label + '.log').read_text(errors='replace')
        iterations = re.findall('^--> iteration (\\d+)', log, re.MULTILINE)
        row['iterations'] = int(iterations[-1]) if iterations else 0
        row['operations'] = {name: log.count(':::' + name + '\n') for name in sorted(set(re.findall('^:::(op__\\w+)', log, re.MULTILINE)))}
    rows[label] = row
record = dict(work=str(work), seconds=a.seconds, original_source_sha256=hashlib.sha256(pathlib.Path('/tmp/wimlib/tests/wlfuzz.c').read_bytes()).hexdigest(), runs=rows, scope='Independent actual random suites; time/PID seeds differ, so operation traces are not matched.')
pathlib.Path(str(prefix) + '.json').write_text(json.dumps(record, indent=2) + '\n')
print(json.dumps(record, indent=2))
sys.exit(0 if all((row.get('status') == 0 for row in rows.values())) else 1)
