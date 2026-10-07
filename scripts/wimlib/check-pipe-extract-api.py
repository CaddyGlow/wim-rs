#!/usr/bin/env python3
"""Compare real pipe ABI calls against a frozen unchanged-header original client."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import tempfile
import threading

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--native', type=Path, default=Path('target/debug/libwim.so'))
parser.add_argument('--output', type=Path, default=Path('docs/wimlib/evidence/native-ffi-pipe-extract/differential.json'))
parser.add_argument('--reference', type=Path, default=Path('docs/wimlib/evidence/native-ffi-pipe-extract/differential-original.json'))
parser.add_argument('--baseline-only', action='store_true', help='Run the original 1080-case matrix only.')
parser.add_argument('--observe-io', action='store_true', help='Count API reads by draining a duplicate only after return.')
parser.add_argument('--recover-only', action='store_true', help='Run only malformed payload/default-versus-recovery cases.')
parser.add_argument('--without-recovery', action='store_true', help='Retain the 1836-case lifecycle matrix without recovery extensions.')
parser.add_argument('--policy-only', action='store_true', help='Compare Linux flag/target failure precedence using the IO observer.')
args = parser.parse_args()
if args.recover_only and (args.baseline_only or args.without_recovery):
    parser.error('--recover-only cannot be combined with --baseline-only or --without-recovery')
if args.policy_only:
    if args.baseline_only or args.recover_only:
        parser.error('--policy-only cannot be combined with --baseline-only or --recover-only')
    args.observe_io = True
fixtures = Path('crates/wim-format/tests/fixtures')
environment = dict(os.environ, WIMLIB_DISABLE_CPU_FEATURES='sse4.2')


def freeze(source, folder):
    folder.mkdir()
    shutil.copyfile(source.resolve(), folder / 'libwim.so')
    (folder / 'libwim.so.15').symlink_to('libwim.so')
    return hashlib.sha256((folder / 'libwim.so').read_bytes()).hexdigest()


def compile_client(folder, client):
    source = ('probe-pipe-extract-policy.c' if args.policy_only else
              'probe-pipe-extract-io.c' if args.observe_io else 'probe-pipe-extract.c')
    return subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I/tmp/wimlib/include',
                           'scripts/wimlib/' + source, '-L' + str(folder),
                           '-Wl,-rpath,' + str(folder), '-lwim', '-o', str(client)],
                          capture_output=True, text=True)


def snapshot(target):
    if not target.exists() and not target.is_symlink():
        return {'created': False, 'entries': [], 'hardlinks': []}
    entries = []
    inodes = {}
    for path in [target, *sorted(target.rglob('*'))]:
        info = path.lstat()
        name = '.' if path == target else str(path.relative_to(target))
        row = {'path': name, 'mode': stat.S_IMODE(info.st_mode)}
        if stat.S_ISLNK(info.st_mode):
            row.update(kind='symlink', target=os.readlink(path))
        elif stat.S_ISDIR(info.st_mode):
            row.update(kind='directory')
        elif stat.S_ISREG(info.st_mode):
            row.update(kind='file', size=info.st_size,
                       sha256=hashlib.sha256(path.read_bytes()).hexdigest(), nlink=info.st_nlink)
            inodes.setdefault((info.st_dev, info.st_ino), []).append(name)
        else:
            row.update(kind='other', file_type=stat.S_IFMT(info.st_mode))
        entries.append(row)
    return {'created': True, 'entries': entries,
            'hardlinks': sorted(sorted(group) for group in inodes.values() if len(group) > 1)}


def remove_target(target):
    if target.is_symlink() or target.is_file():
        target.unlink()
    elif target.exists():
        shutil.rmtree(target)


def execute(client, case, data, target):
    remove_target(target)
    auxiliary = target.with_name(target.name + '-linked')
    remove_target(auxiliary)
    argument = str(target)
    policy = case.get('target_policy')
    if policy == 'null':
        argument = '@NULL'
    elif policy == 'empty':
        argument = '@EMPTY'
    elif policy == 'missing-parent':
        argument = str(target / 'missing' / 'target')
    elif policy == 'existing-file':
        target.write_bytes(b'preserve existing test target\n')
    elif policy in ('existing-dir', 'nonempty-dir', 'readonly-dir'):
        target.mkdir()
        if policy == 'nonempty-dir':
            (target / 'sentinel.bin').write_bytes(b'preserve existing test sentinel\n')
        if policy == 'readonly-dir':
            target.chmod(0o500)
    elif policy in ('directory-symlink', 'dangling-symlink'):
        if policy == 'directory-symlink':
            auxiliary.mkdir()
        target.symlink_to(auxiliary, target_is_directory=True)
    env = dict(environment, PIPE_FIXTURE_SIZE=str(len(data)))
    process = subprocess.Popen([str(client), case['image'], argument,
                                str(case['flags']), str(case['stop'])], stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, bufsize=0)
    channel = process.stdin
    process.stdin = None
    write_errors = []

    def feed():
        try:
            for start in range(0, len(data), case['fragment']):
                chunk = memoryview(data)[start:start + case['fragment']]
                while chunk:
                    count = channel.write(chunk)
                    chunk = chunk[count:]
        except BrokenPipeError:
            # Early termination is expected when the API owns/closes its input.
            pass
        except OSError as error:
            write_errors.append({'errno': error.errno, 'message': str(error)})
        finally:
            channel.close()

    writer = threading.Thread(target=feed, daemon=True)
    writer.start()
    try:
        stdout, stderr = process.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate()
        write_errors.append({'timeout': True})
    writer.join(timeout=2)
    row = {'returncode': process.returncode, 'stdout': stdout.decode(errors='backslashreplace'),
           'stderr': stderr.decode(errors='backslashreplace'), 'tree': snapshot(target)}
    if policy in ('directory-symlink', 'dangling-symlink'):
        row['tree']['linked_destination'] = snapshot(auxiliary)
    if write_errors:
        row['writer_errors'] = write_errors
    if policy == 'readonly-dir':
        target.chmod(0o700)
    remove_target(target)
    remove_target(auxiliary)
    return row


def metadata_locations(data):
    final = data[-208:]
    offset = int.from_bytes(final[56:64], 'little')
    size = int.from_bytes(final[48:55], 'little')
    return [int.from_bytes(data[start + 8:start + 16], 'little')
            for start in range(offset, offset + size, 50) if data[start + 7] & 2]


def first_payload(data):
    final = data[-208:]
    offset = int.from_bytes(final[56:64], 'little')
    size = int.from_bytes(final[48:55], 'little')
    for start in range(offset, offset + size, 50):
        if data[start + 7] & 2 == 0:
            return {'offset': int.from_bytes(data[start + 8:start + 16], 'little'),
                    'size': int.from_bytes(data[start:start + 7], 'little'),
                    'flags': data[start + 7],
                    'usize': int.from_bytes(data[start + 16:start + 24], 'little')}
    raise ValueError('fixture has no payload resource')


with tempfile.TemporaryDirectory(prefix='wim-pipe-differential-') as directory:
    root = Path(directory)
    original_sha = freeze(Path('/tmp/wimlib-native-oracle/.libs/libwim.so'), root / 'original-library')
    native_path = args.native / 'libwim.so' if args.native.is_dir() else args.native
    native_sha = freeze(native_path, root / 'native-library')
    original = root / 'original-client'
    native = root / 'native-client'
    built = compile_client(root / 'original-library', original)
    if built.returncode:
        raise RuntimeError(built.stderr)
    native_build = compile_client(root / 'native-library', native)
    pipable = (fixtures / 'pipable-resource.wim').read_bytes()
    payloads = {'pipable': pipable, 'ordinary': (fixtures / 'xpress-resource.wim').read_bytes(),
                'empty': b'', 'header-short': pipable[:100], 'xml-short': pipable[:250]}

    def changed(name, offset, replacement):
        value = bytearray(pipable)
        value[offset:offset + len(replacement)] = replacement
        payloads[name] = bytes(value)

    changed('part-two-first', 40, struct.pack('<HH', 2, 2))
    changed('image-count-two', 44, struct.pack('<I', 2))
    changed('invalid-xml-frame-magic', 208, b'badframe')
    changed('xml-not-metadata', 244, struct.pack('<I', 0))
    changed('xml-empty', 216, struct.pack('<Q', 0))
    changed('xml-hash-zero', 224, bytes(20))
    changed('xml-invalid-text', 248, b'\xff\xff\xff\xff')
    cases = []

    def add(layout, image, flags, stop, fragment=65536):
        cases.append({'layout': layout, 'image': image, 'flags': flags,
                      'stop': stop, 'fragment': fragment})

    for values in itertools.product(payloads, ['NULL', '1', '0', 'all', 'missing'],
                                    [0, 4, -1], [-1, 0, 103, 105, 107, 203]):
        add(*values)
    if args.policy_only:
        cases.clear()
        # Source check_extract_flags() validates combinations/platform gates only
        # after pipe preflight; public-mask rejection occurs before fd ownership.
        flags = [0, 1, 2, 4, 8, 16, 32, 64, 128, 192, 256, 512, 768, 1024,
                 2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144, 524288,
                 1048576, 2097152, 4194304, 8388608, 16777216, 33554432,
                 67108864, 134217728, 193, 769, 1025, 2097153, 18874368, -1]
        policies = ['new-dir', 'null', 'empty', 'missing-parent', 'existing-file',
                    'existing-dir', 'nonempty-dir', 'readonly-dir',
                    'directory-symlink', 'dangling-symlink']
        for layout, image, flag, stop, policy in itertools.product(
                ['pipable', 'header-short', 'ordinary'], ['1', 'missing'], flags, [0, 103], policies):
            add(layout, image, flag, stop, 7)
            cases[-1]['target_policy'] = policy
    elif not args.baseline_only:
        for name in ['none', 'lzx', 'lzms']:
            payloads[name] = (fixtures / ('pipe-' + name + '.wim')).read_bytes()
            for image, flags, stop in itertools.product(['NULL', '1'], [0, 4], [-1, 0, 100, 103, 104, 105, 106, 107, 200, 203, 204, 206]):
                add(name, image, flags, stop)
        for layout, image, stop, fragment in itertools.product(['pipable', 'none', 'lzx', 'lzms'],
                                                              ['NULL', '1'], [0, 100, 103, 104, 105, 106, 107, 204, 206], [1, 7, 1024]):
            add(layout, image, 0, stop, fragment)
        for layout in ['xpress-64k', 'lzx-64k', 'lzms-128k']:
            payloads[layout] = (fixtures / ('pipe-' + layout + '.wim')).read_bytes()
            for image, stop, fragment in itertools.product(['NULL', '1'], [0, 104, 204, 107], [1, 7, 65536]):
                add(layout, image, 0, stop, fragment)
        two = (fixtures / 'pipe-two-image.wim').read_bytes()
        start = metadata_locations(two)[1]
        compressed_size = int.from_bytes(two[start:start + 4], 'little')
        variants = {'two-images': two,
                    'unselected-zero-metadata': two[:start + 4] + bytes(compressed_size) + two[start + 4 + compressed_size:],
                    'unselected-invalid-huffman': two[:start + 4] + b'\x11' * compressed_size + two[start + 4 + compressed_size:],
                    'unselected-zero-chunk': two[:start] + bytes(4) + two[start + 4:],
                    'unselected-solid-only': two[:start - 4] + struct.pack('<I', 18) + two[start:]}
        payloads.update(variants)
        for layout, image, stop, fragment in itertools.product(variants, ['1', '2'], [0, 100, 103, 104, 105, 106, 107, 204, 206], [7, 65536]):
            add(layout, image, 0, stop, fragment)
        # Independent original-created filesystem fixture includes real hardlinks,
        # directory/file Unix modes and a relative symlink, not just one payload.
        source = root / 'rich-source'
        (source / 'nested').mkdir(parents=True)
        (source / 'nested/data.bin').write_bytes(bytes(range(256)) * 300 + b'last chunk')
        (source / 'nested').chmod(0o750)
        (source / 'nested/data.bin').chmod(0o640)
        os.link(source / 'nested/data.bin', source / 'alias')
        (source / 'link').symlink_to('nested/data.bin')
        for codec in ['none', 'XPRESS', 'LZX', 'LZMS']:
            output = root / ('rich-' + codec + '.wim')
            command = ['/tmp/wimlib-native-oracle/wimlib-imagex', 'capture', str(source), str(output),
                       '--compress=' + codec, '--pipable', '--unix-data', '--no-acls', '--nocheck', '--threads=1']
            generated = subprocess.run(command, env=environment, capture_output=True, text=True)
            if generated.returncode:
                raise RuntimeError(generated.stderr)
            layout = 'rich-' + codec.lower()
            payloads[layout] = output.read_bytes()
            for flags, stop, fragment in itertools.product([0, 32], [0, 100, 103, 104, 105, 106, 107, 204, 206], [7, 65536]):
                add(layout, '1', flags, stop, fragment)
        if args.recover_only:
            cases.clear()
        for codec in ([] if args.without_recovery else ['none', 'pipable', 'lzx', 'lzms']):
            data = payloads[codec]
            resource = first_payload(data)
            start = resource['offset']
            variants = {
                'frame-metadata': data[:start - 4] + struct.pack('<I', resource['flags'] | 2) + data[start:],
                'frame-zero-size': data[:start - 32] + bytes(8) + data[start - 24:],
                'frame-magic': data[:start - 40] + b'badframe' + data[start - 32:],
                'resource-truncated': data[:start + resource['size'] - 1],
            }
            if resource['flags'] & 4:
                length = int.from_bytes(data[start:start + 4], 'little')
                variants.update({
                    'body-fill-11': data[:start + 4] + b'\x11' * length + data[start + 4 + length:],
                    'body-fill-zero': data[:start + 4] + bytes(length) + data[start + 4 + length:],
                    'chunk-zero-size': data[:start] + bytes(4) + data[start + 4:],
                    'chunk-oversize': data[:start] + b'\xff' * 4 + data[start + 4:],
                    'chunk-prefix-short': data[:start + 2],
                    'chunk-body-short': data[:start + 4 + length // 2],
                })
            else:
                variants.update({
                    'body-hash-mismatch': data[:start] + bytes([data[start] ^ 0x80]) + data[start + 1:],
                    'invalid-compressed-flag': data[:start - 4] + struct.pack('<I', 4) + data[start:],
                })
            for name, value in variants.items():
                layout = 'recover-' + codec + '-' + name
                payloads[layout] = value
                for flags, stop, fragment in itertools.product([0, 2], [0, 104, 204], [7, 65536]):
                    add(layout, '1', flags, stop, fragment)
    common = {'source_commit': 'cd5e231c348c255ae5088873b5a66ee0eb96fa07',
              'original_sha256': original_sha, 'native_sha256': native_sha,
              'observe_io': args.observe_io,
              'oracle_environment': {'WIMLIB_DISABLE_CPU_FEATURES': 'sse4.2'},
              'payload_sha256': {name: hashlib.sha256(value).hexdigest() for name, value in payloads.items()}}
    observations = []
    for index, case in enumerate(cases):
        observations.append({'case': case, 'original': execute(original, case, payloads[case['layout']], root / ('target-' + str(index)))})
    args.reference.parent.mkdir(parents=True, exist_ok=True)
    args.reference.write_text(json.dumps(dict(common, scope='original reference before native comparison',
                                              cases=len(cases), results=observations), indent=2) + '\n')
    if native_build.returncode:
        result = dict(common, cases=len(cases), exact=0, native_link_error=native_build.stderr, results=observations)
    else:
        exact = exact_stderr = 0
        for index, row in enumerate(observations):
            case = row['case']
            row['native'] = execute(native, case, payloads[case['layout']], root / ('target-' + str(index)))
            row['equal_contract'] = (row['original']['returncode'] == row['native']['returncode'] == 0
                                     and not row['original'].get('writer_errors') and not row['native'].get('writer_errors')
                                     and all(row['original'][key] == row['native'][key] for key in ['stdout', 'tree']))
            row['equal_stderr'] = row['original']['stderr'] == row['native']['stderr']
            exact += row['equal_contract']
            exact_stderr += row['equal_stderr']
        result = dict(common, cases=len(cases), exact=exact, exact_stderr=exact_stderr, results=observations)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key != 'results' and key != 'payload_sha256'}, indent=2))
