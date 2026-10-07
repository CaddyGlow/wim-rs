#!/usr/bin/env python3
"""Verify both complete WinRE WIMs and compare every logical stream and attribute."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('layout', Path(__file__).with_name('check-qcow2-reparse-layout.py'))
layout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layout)


def sha(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def artifact(path):
    return {'path': str(path.resolve()), 'sha256': sha(path)}


def write(path, data):
    with path.open('xb') as target:
        target.write(data)


def inventory(wim, label, args):
    log = args.output_dir / (label + '-verify.log')
    command = [str(args.wimlib), 'verify', str(wim)]
    with log.open('xb') as output:
        result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT, check=False)
        output.write(('\nIndependent full verification exit code: %d\n' % result.returncode).encode())
    if result.returncode:
        raise ValueError('full archive verification failed: ' + str(log))
    archive = layout.Archive(wim, args.library)
    cache, entries, kernel = {}, [], None
    try:
        xml_data = archive.resource(layout.descriptor(archive.read(72, 24)))
        xml = ET.fromstring(xml_data)
        images = xml.findall('IMAGE')
        if len(images) != 1:
            raise ValueError('WinRE must contain exactly one image')
        windows = images[0].find('WINDOWS')
        if windows is None:
            raise ValueError('missing Windows identity')
        identity = {key: int(windows.findtext(field)) for key, field in [('architecture', 'ARCH'), ('build', 'VERSION/BUILD'), ('revision', 'VERSION/SPBUILD')]}
        for path, attrs, main, extras, _ in archive.nodes():
            streams, unnamed = [], 0
            for digest, name in ([(main, '')] if main != layout.ZERO else []) + extras:
                kind = 'REPARSE' if not name and attrs & 1024 and unnamed == 0 else 'DATA'
                if not name:
                    unnamed += 1
                if digest not in cache:
                    content = archive.blob(digest)
                    cache[digest] = (len(content), hashlib.sha256(content).hexdigest())
                size, digest256 = cache[digest]
                streams.append({'kind': kind, 'name_utf16_le': name.encode('utf-16-le', 'surrogatepass').hex(), 'size': size, 'sha256': digest256})
                if path.casefold() == '/windows/system32/ntoskrnl.exe' and kind == 'DATA' and not name:
                    kernel = archive.blob(digest)
            if not attrs & 0x410 and not any(s['kind'] == 'DATA' and not s['name_utf16_le'] for s in streams):
                streams.append({'kind': 'DATA', 'name_utf16_le': '', 'size': 0, 'sha256': hashlib.sha256(b'').hexdigest()})
            canonical = path or '/'
            entries.append({'path_utf16_le': canonical.encode('utf-16-le', 'surrogatepass').hex(), 'attributes': attrs, 'streams': sorted(streams, key=lambda s: (s['name_utf16_le'], s['kind']))})
        keys = [row['path_utf16_le'] for row in entries]
        if len(set(keys)) != len(keys):
            raise ValueError('duplicate canonical metadata paths')
        if len(entries) != int(images[0].findtext('DIRCOUNT')) + int(images[0].findtext('FILECOUNT')) + 1:
            raise ValueError('XML file/directory totals disagree with complete tree including root')
        if kernel is None or kernel[:2] != b'MZ':
            raise ValueError('missing valid Windows kernel')
        pe = layout.u32(kernel, 60)
        if kernel[pe:pe + 4] != b'PE\0\0' or layout.u16(kernel, pe + 4) != 0x8664:
            raise ValueError('kernel is not AMD64 PE')
        identity['kernel_sha256'] = hashlib.sha256(kernel).hexdigest()
        manifest = {'schema': 1, 'kind': 'winre-content-manifest', 'entries': sorted(entries, key=lambda e: e['path_utf16_le'])}
        files = {'wim': wim, 'verify': log, 'manifest': args.output_dir / (label + '-manifest.json'), 'xml': args.output_dir / (label + '-xml.xml'), 'kernel': args.output_dir / (label + '-ntoskrnl.exe')}
        write(files['manifest'], (json.dumps(manifest, ensure_ascii=True, separators=(',', ':')) + '\n').encode())
        write(files['xml'], xml_data)
        write(files['kernel'], kernel)
        return manifest, identity, {label + '_' + key: artifact(value) for key, value in files.items()}, {'command': command, 'exit_code': result.returncode}
    finally:
        archive.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('target', type=Path)
    parser.add_argument('--library', required=True, type=Path)
    parser.add_argument('--wimlib', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    source, source_id, source_art, source_run = inventory(args.source, 'source', args)
    target, target_id, target_art, target_run = inventory(args.target, 'target', args)
    a = {row['path_utf16_le']: row for row in source['entries']}
    b = {row['path_utf16_le']: row for row in target['entries']}
    differences = [{'path_utf16_le': key, 'source': a.get(key), 'target': b.get(key)} for key in sorted(a.keys() | b.keys()) if a.get(key) != b.get(key)]
    if source_id != target_id:
        differences.append({'identity': {'source': source_id, 'target': target_id}})
    report = {'schema': 1, 'kind': 'winre-full-equivalence', 'source_sha256': source_art['source_wim']['sha256'], 'target_sha256': target_art['target_wim']['sha256'], 'entry_count': len(a), 'stream_count': sum(len(row['streams']) for row in a.values()), 'differences': differences, 'source_identity': source_id, 'target_identity': target_id, 'artifacts': source_art | target_art, 'provenance': {'wimlib': artifact(args.wimlib), 'library': artifact(args.library), 'producer': artifact(Path(__file__)), 'source_verify': source_run, 'target_verify': target_run, 'wimlib_version': subprocess.run([str(args.wimlib), '--version'], capture_output=True, text=True, check=True).stdout.strip()}}
    write(args.output_dir / 'equivalence.json', (json.dumps(report, ensure_ascii=True, indent=2) + '\n').encode())
    print(json.dumps({'entries': len(a), 'streams': report['stream_count'], 'differences': len(differences)}))
    raise SystemExit(bool(differences))


if __name__ == '__main__':
    main()
