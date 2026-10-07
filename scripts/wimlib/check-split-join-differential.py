#!/usr/bin/env python3
"""Original-library split/join/apply oracle for native in-memory operations."""
import hashlib
import json
import pathlib
import subprocess
import tempfile
import argparse

parser = argparse.ArgumentParser()
parser.add_argument('--oracle', default='/tmp/wimlib-native-oracle/wimlib-imagex')
parser.add_argument('--native', default='target/debug/examples/split_join')
parser.add_argument('--output', required=True)
args = parser.parse_args()
native = str(pathlib.Path(args.native).resolve())

def run(*cmd):
    result = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError(f'{cmd}: {result.returncode}\n{result.stdout.decode()}\n{result.stderr.decode()}')
    return result.stdout.decode()

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

observations = []
with tempfile.TemporaryDirectory(prefix='native-split-join-') as name:
    root = pathlib.Path(name)
    source = root / 'source'
    source.mkdir()
    for i in range(4):
        (source / f'file{i}').write_bytes(bytes((n * 73 + i * 29 + n // 256) % 256 for n in range(180000 + i * 41)))
    expected = {p.name: digest(p) for p in source.iterdir()}
    for compression in ['none', 'XPRESS', 'LZX', 'LZMS']:
        for pipable in [False, True]:
            label = f'{compression}-{"pipable" if pipable else "ordinary"}'
            case = root / label
            case.mkdir()
            original = case / 'source.wim'
            command = [args.oracle, 'capture', str(source), str(original), '--compress=' + compression, '--check']
            if pipable:
                command += ['--pipable']
            run(*command)
            # A tiny target makes metadata its own part and each oversized blob
            # indivisible. It exercises spanned references rather than copies.
            count = int(run(native, 'split', str(original), '1', str(case / 'native')))
            assert count == 5, (label, count)
            parts = [case / f'native{i}.swm' for i in range(1, count + 1)]
            joined = case / 'original-joined.wim'
            run(args.oracle, 'join', str(joined), *map(str, reversed(parts)), '--check')
            applied = case / 'original-applied'
            run(args.oracle, 'apply', str(joined), '1', str(applied))
            assert {p.name: digest(p) for p in applied.iterdir()} == expected
            # Split with the original and join with Rust, in reverse order.
            original_part = case / 'original.swm'
            run(args.oracle, 'split', str(original), str(original_part), '0.000001')
            original_parts = sorted(case.glob('original*.swm'))
            native_joined = case / 'native-joined.wim'
            run(native, 'join', str(native_joined), *map(str, reversed(original_parts)))
            run(args.oracle, 'verify', str(native_joined))
            native_applied = case / 'native-applied'
            run(args.oracle, 'apply', str(native_joined), '1', str(native_applied))
            assert {p.name: digest(p) for p in native_applied.iterdir()} == expected
            invalid = {}
            for fault, bad_parts in [('missing', parts[:-1]), ('duplicate', [parts[0], parts[0], *parts[2:]])]:
                result = subprocess.run([args.oracle, 'join', str(case / (fault + '.wim')), *map(str, bad_parts)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                assert result.returncode == 62, (label, fault, result.returncode)
                invalid[fault] = result.returncode
            foreign = case / 'foreign.swm'
            foreign_bytes = bytearray(parts[-1].read_bytes())
            foreign_bytes[24] ^= 1  # first GUID byte in the fixed disk header
            foreign.write_bytes(foreign_bytes)
            result = subprocess.run([args.oracle, 'join', str(case / 'foreign.wim'), *map(str, parts[:-1]), str(foreign)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert result.returncode == 62, (label, 'foreign-guid', result.returncode)
            invalid['foreign_guid'] = result.returncode
            observations.append({'case': label, 'native_parts': count, 'original_parts': len(original_parts), 'original_join_apply': 'pass', 'native_join_verify_apply': 'pass', 'original_invalid_sets': invalid})
pathlib.Path(args.output).write_text(json.dumps({'original_library': 'wimlib 1.14.5', 'cases': observations}, indent=2) + '\n')
print(f'{len(observations)} split/join bidirectional cases passed')
