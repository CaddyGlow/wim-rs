#!/usr/bin/env python3
"""Independent whole-WIM/resource and raw-layout gates for the fresh Setup profile.

Uses upstream wimlib only as a test oracle; production capture/media stay in Rust.
Outputs a new cache directory; refuses to replace prior evidence.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

from pe_inspect import inspect_pe

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('raw_layout', HERE / 'check-qcow2-reparse-layout.py')
layout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout)


def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as source:
        for block in iter(lambda: source.read(8 * 1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def recovery_configuration(path, native_baseline):
    """Compare native schema coverage and require unbound fresh-install state."""
    actual = ET.parse(path).getroot()
    native = ET.parse(native_baseline).getroot()
    expected_schema = {node.tag: sorted(node.attrib) for node in native}
    schema = {node.tag: sorted(node.attrib) for node in actual}
    errors = []
    if actual.tag != native.tag or actual.attrib != native.attrib:
        errors.append('root schema differs from native Windows configuration')
    if schema != expected_schema or len(list(actual)) != len(expected_schema):
        errors.append('known element/attribute coverage differs from native Windows configuration')
    zero = '{00000000-0000-0000-0000-000000000000}'
    values = {'WinreBCD': {'id': zero}, 'WinREStaged': {'state': '0'},
              'InstallState': {'state': '0'}, 'OsInstallAvailable': {'state': '0'},
              'CustomImageAvailable': {'state': '0'}, 'OperationParam': {'path': ''},
              'OsBuildVersion': {'path': ''}, 'ScheduledOperation': {'state': '4'}}
    for name in ('WinreLocation', 'ImageLocation', 'PBRImageLocation',
                 'PBRCustomImageLocation', 'DownlevelWinreLocation'):
        values[name] = {'path': '', 'id': '0', 'offset': '0', 'guid': zero}
        if name.startswith('PBR'):
            values[name]['index'] = '0'
    for name, attributes in values.items():
        node = actual.find(name)
        if node is None or node.attrib != attributes:
            errors.append(name + ': fresh-install state or binding differs')
    return {'passes': not errors, 'errors': errors, 'native_baseline_sha256': sha(native_baseline),
            'configuration_sha256': sha(path), 'schema': schema,
            'xml': ET.tostring(actual, encoding='unicode')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wim', required=True, type=Path)
    parser.add_argument('--capture-report', required=True, type=Path)
    parser.add_argument('--source-metadata', required=True, type=Path)
    parser.add_argument('--source-reagent', required=True, type=Path)
    parser.add_argument('--setup-system-drive', choices=['C:'])
    parser.add_argument('--source-oobe-task', type=Path)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--wimlib', required=True, type=Path)
    parser.add_argument('--library', required=True, type=Path)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=False)
    p = args.evidence
    report = json.loads(args.capture_report.read_text())
    commands = []

    def run(label, arguments):
        command = [str(args.wimlib), *map(str, arguments)]
        with (p / (label + '.log')).open('wb') as output:
            result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT)
        commands.append({'command': command, 'exit': result.returncode, 'log': label + '.log'})
        if result.returncode:
            raise RuntimeError(label + ' failed; retained ' + str(p / (label + '.log')))
        print(label + ' completed', flush=True)

    raw_command = ['python3', str(HERE / 'check-qcow2-reparse-layout.py'), str(args.wim),
                   '--library', str(args.library), '--source-metadata', str(args.source_metadata),
                   '--output', str(p / 'reparse-layout.json'),
                   '--absent-path', '/Windows.old',
                   '--absent-path', '/Windows/Panther/unattend.xml',
                   '--absent-path', '/Windows/Panther/unattend-original.xml']
    if args.setup_system_drive:
        raw_command.extend(['--setup-system-drive', args.setup_system_drive])
    with (p / 'raw-layout.log').open('wb') as output:
        result = subprocess.run(raw_command, stdout=output, stderr=subprocess.STDOUT)
    commands.append({'command': raw_command, 'exit': result.returncode, 'log': 'raw-layout.log'})
    if result.returncode:
        raise RuntimeError('raw layout/deployment artifacts failed; evidence retained')
    print('raw layout, absent Windows.old and both absent cached answers passed', flush=True)
    run('verify', ['verify', args.wim])
    run('info', ['info', args.wim, '--extract-xml', p / 'install.xml'])
    run('fixture-detailed', ['dir', args.wim, '1', '--path=/NativeDiskCapture', '--detailed'])
    run('extract', ['extract', args.wim, '1', 'Windows/System32/Recovery/Winre.wim',
                    'Windows/System32/Recovery/ReAgent.xml',
                    'Windows/System32/ntoskrnl.exe', '--dest-dir=' + str(p)])
    recovery_config = recovery_configuration(p / 'ReAgent.xml', args.source_reagent)
    (p / 'recovery-configuration.json').write_text(json.dumps(recovery_config, indent=2) + '\n')
    run('verify-winre', ['verify', p / 'Winre.wim'])
    source = json.loads(args.source_metadata.read_text())
    if isinstance(source, dict) and 'stdout' in source:
        source = json.loads(source['stdout'])
    rows = {'/NativeDiskCapture' + row['Path'].replace('\\', '/'): row for row in source}
    stream_evidence = []
    fixture_errors = []
    actual_groups = {}
    expected_groups = {}
    seen_fixture = set()
    oobe_task = None
    archive = layout.Archive(args.wim, args.library)
    try:
        for path, attrs, main_hash, extra, inode in archive.nodes():
            if path.casefold() == '/windows/system32/tasks/microsoft/windows/cloudexperiencehost/createobjecttask':
                oobe_task = archive.blob(main_hash)
            if path not in rows:
                continue
            seen_fixture.add(path)
            baseline = rows[path]
            expected_groups.setdefault(baseline['Info']['Identity'], []).append(path)
            actual_groups.setdefault(('inode', inode) if inode and not attrs & 0x400 else ('path', path), []).append(path)
            streams = list(extra)
            if attrs & 0x400:
                streams = streams[1:] if streams else []
            elif main_hash != layout.ZERO:
                streams.insert(0, (main_hash, ''))
            elif not attrs & 0x10 and not any(not name for _, name in streams):
                streams.insert(0, (layout.ZERO, ''))
            if len(streams) != len(baseline['Streams']):
                fixture_errors.append(path + ': stream count differs')
            for stream in baseline['Streams']:
                digest = next((h for h, name in streams if name == stream['Name']), None)
                if digest is None:
                    fixture_errors.append(path + ': missing stream ' + repr(stream['Name']))
                    continue
                payload = archive.blob(digest)
                observed = hashlib.sha256(payload).hexdigest() if payload else ''
                expected = stream['Hash'].replace('-', '').lower()
                passes = observed == expected and len(payload) == stream['Length']
                if not passes:
                    fixture_errors.append(path + ': incorrect stream ' + repr(stream['Name']))
                stream_evidence.append({'path': path, 'name': stream['Name'], 'bytes': len(payload),
                                        'sha256': observed, 'expected_sha256': expected, 'passes': passes})
    finally:
        archive.close()
    if set(rows) != seen_fixture:
        fixture_errors.append('fixture path coverage differs')
    observed_groups = sorted(sorted(paths) for paths in actual_groups.values())
    source_groups = sorted(sorted(paths) for paths in expected_groups.values())
    if observed_groups != source_groups:
        fixture_errors.append('hardlink topology differs')
    (p / 'fixture-streams.json').write_text(json.dumps({'streams': stream_evidence, 'errors': fixture_errors,
                                                     'hardlink_groups': observed_groups}, indent=2) + '\n')
    image = ET.parse(p / 'install.xml').getroot()
    windows = image.find('IMAGE/WINDOWS')
    metadata_warnings = sum([re.findall(r'\[WARNING\] ([^\r\n]+)', (p / name).read_text())
                             for name in ('verify.log', 'info.log', 'fixture-detailed.log')], [])
    machine = inspect_pe(p / 'ntoskrnl.exe')['machine']
    sources = [{'path': item['path'], 'expected_sha256': item['sha256'], 'actual_sha256': sha(item['path'])}
               for item in report['sources']]
    digest = sha(args.wim)
    recovery = sha(p / 'Winre.wim')
    identity = report['windows']
    raw = json.loads((p / 'reparse-layout.json').read_text())
    gates = {'full_wim_verify': True, 'winre_verify': True, 'metadata_reader_warning_free': not metadata_warnings,
             'native_recovery_schema_and_unbound_state': recovery_config['passes'],
             'no_captured_windows_old': '/Windows.old' in raw['forbidden_paths'] and '/Windows.old' not in raw['forbidden_paths_found'],
             'raw_reparse_stream_layout_and_no_cached_answers': raw['passes'],
             'all_fixture_streams_and_hardlinks': not fixture_errors,
             'wim_sha256': digest == report['wim_sha256'], 'winre_sha256': recovery == report['winre_sha256'],
             'source_hashes_preserved': all(item['actual_sha256'] == item['expected_sha256'] for item in sources),
             'one_image': len(image.findall('IMAGE')) == 1,
             'entry_count': int(image.findtext('IMAGE/DIRCOUNT')) + int(image.findtext('IMAGE/FILECOUNT')) == report['entries'],
             'architecture': int(windows.findtext('ARCH')) == identity['architecture'] == 9 and machine == 0x8664,
             'build': int(windows.findtext('VERSION/BUILD')) == identity['build'],
             'revision': int(windows.findtext('VERSION/SPBUILD')) == identity['revision'],
             'edition': windows.findtext('EDITIONID') == identity['edition'],
             'language': windows.findtext('LANGUAGES/DEFAULT') == identity['language']}
    task_evidence = None
    if args.source_oobe_task:
        expected_task = args.source_oobe_task.read_bytes()
        task_enabled = False
        if oobe_task:
            task_root = ET.fromstring(oobe_task)
            task_enabled = task_root.findtext('{*}Settings/{*}Enabled', default='true').strip().lower() == 'true'
        gates['oobe_task_source_bytes_and_enabled'] = oobe_task == expected_task and task_enabled
        task_evidence = {'source_sha256': sha(args.source_oobe_task),
                         'captured_sha256': hashlib.sha256(oobe_task).hexdigest() if oobe_task else None,
                         'enabled': task_enabled,
                         'absent_enabled_semantics': 'Task Scheduler schema default true'}
    summary = {'oracle': 'upstream wimlib + independent raw metadata parser', 'passes': all(gates.values()),
               'gates': gates, 'commands': commands, 'wim_sha256': digest, 'wim_bytes': args.wim.stat().st_size,
               'winre_sha256': recovery, 'source_hashes': sources, 'fixture_stream_rows': len(stream_evidence),
               'fixture_errors': fixture_errors, 'raw_reparse_entries': len(raw['reparse_entries']),
               'recovery_configuration': recovery_config,
               'oobe_task': task_evidence,
               'metadata_reader_warnings': metadata_warnings, 'kernel_pe_machine': hex(machine),
               'xml_windows': ET.tostring(windows, encoding='unicode'),
               'host_extraction_limitations': re.findall(r'\[WARNING\] ([^\r\n]+)', (p / 'extract.log').read_text()),
               'winre_verification_caveats': re.findall(r'\[WARNING\] ([^\r\n]+)', (p / 'verify-winre.log').read_text()),
               'capture_report_sha256': sha(args.capture_report), 'cache_evidence': str(p.resolve()),
               'evidence_sha256': {file.name: sha(file) for file in p.iterdir() if file.suffix in ('.log', '.xml', '.json')},
               'limitations': ['Archive/source stream verification does not prove successful Windows Setup, NTFS metadata restoration, servicing or WinRE boot.']}
    (p / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps({'passes': summary['passes'], 'gates': gates, 'fixture_stream_rows': len(stream_evidence)}, indent=2), flush=True)
    raise SystemExit(0 if summary['passes'] else 1)


if __name__ == '__main__':
    main()
