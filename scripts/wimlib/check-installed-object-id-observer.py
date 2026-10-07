"""Prepare/check GET-only observations; no source or target disk mutation."""
import argparse
import hashlib
import json
from pathlib import Path


def exact_hex(value, size):
    if not isinstance(value, str) or len(value) != size * 2 or value != value.lower():
        raise ValueError('Noncanonical hex or wrong size')
    return bytes.fromhex(value)


def selector(value):
    raw = exact_hex(value, len(value) // 2)
    if len(raw) % 2:
        raise ValueError('Odd UTF16')
    units = [int.from_bytes(raw[n:n+2], 'little') for n in range(0, len(raw), 2)]
    if not units or units[0] != 47 or any(u in (0, 58, 92) for u in units):
        raise ValueError('Selector escape')
    parts = value  # Preserve original UTF16, including unpaired surrogates.
    components = []
    start = 1
    for n in range(1, len(units) + 1):
        if n == len(units) or units[n] == 47:
            components.append(units[start:n]); start = n + 1
    if any(p in ([], [46], [46, 46]) for p in components):
        raise ValueError('Selector component escape')
    return parts


def source_rows(inventory):
    rows = []
    seen = set()
    for group in inventory['groups']:
        exact_hex(group['object_id_full64'], 64)
        for path in group['paths']:
            raw = selector(path['path_utf16_le'])
            if raw in seen:
                raise ValueError('Duplicate source selector')
            seen.add(raw)
            rows.append(dict(path_utf16_le=raw, source_full64=group['object_id_full64'],
                             source_file_reference=group['file_reference']))
    if len(rows) != inventory['object_id_paths'] or len(inventory['groups']) != inventory['object_id_file_groups']:
        raise ValueError('Incomplete source scope')
    return rows


def metadata_shape(value):
    if not isinstance(value, dict):
        raise ValueError('Metadata must be an object')
    for field in ('creation_time', 'access_time', 'write_time', 'change_time', 'attributes'):
        maximum = 2**32 if field == 'attributes' else 2**64
        if type(value.get(field)) is not int or not 0 <= value[field] < maximum:
            raise ValueError('Invalid metadata integer: ' + field)
    if not isinstance(value.get('security_raw'), str):
        raise ValueError('Security buffer must be hex text')
    raw = exact_hex(value['security_raw'], len(value['security_raw']) // 2)
    if len(raw) < 20 or raw[0] != 1 or not int.from_bytes(raw[2:4], 'little') & 0x8000:
        raise ValueError('Invalid self-relative security header')
    for offset in (4, 8, 12, 16):
        start = int.from_bytes(raw[offset:offset+4], 'little')
        if start and not 20 <= start < len(raw):
            raise ValueError('Security component offset outside buffer')
    # Header bounds only, not full SID/ACL/ACE structural validation.


def validate_report(inputs, report):
    if report['mode'] != 'GET-only-installed-objectids' or report['production_executable'] is not False:
        raise ValueError('Observer mode')
    if report['target_volume_object_id_bound_context'] != inputs['target_volume_object_id'] or report['target_volume_object_id_observed_by_this_helper'] is not False:
        raise ValueError('Raw-context volume binding mismatch')
    for key in ('source_inventory_sha256', 'capture_wim_sha256', 'target_snapshot_sha256', 'target_context_receipt_sha256', 'observer_helper_sha256'):
        if report[key] != inputs[key]:
            raise ValueError('Provenance mismatch: ' + key)
    if len(report['rows']) != len(inputs['rows']):
        raise ValueError('Missing observation')
    identities = {}
    owners = {}
    for expected, actual in zip(inputs['rows'], report['rows']):
        if type(actual.get('source_file_reference')) is not int or actual.get('source_file_reference') != expected['source_file_reference'] or actual.get('source_full64') != expected['source_full64']:
            raise ValueError('Source row binding mismatch')
        if actual.get('error') or actual['path_utf16_le'] != expected['path_utf16_le']:
            raise ValueError('Missing/changed selector')
        if actual['identity_observation'] != 'WindowsFileIdInfo':
            raise ValueError('Synthetic identity forbidden')
        if actual['first_file_id_info24'] != actual['file_id_info24'] or actual['first_object_id_full64'] != actual['object_id_full64']:
            raise ValueError('Close/reopen identity or full64 changed')
        identity = exact_hex(actual['file_id_info24'], 24)
        objectid = exact_hex(actual['object_id_full64'], 64)
        if int.from_bytes(identity[:8], 'little') != inputs['target_volume_serial'] or not any(identity[8:]):
            raise ValueError('Target volume/identity mismatch')
        if objectid[:16] != exact_hex(expected['source_full64'], 64)[:16]:
            raise ValueError('Existing ObjectID16 changed')
        metadata = actual['metadata']
        metadata_shape(metadata)
        for before in ('first_metadata', 'first_after_get_metadata', 'reopened_after_get_metadata'):
            metadata_shape(actual[before])
            if actual[before] != metadata:
                raise ValueError('GET changed metadata: ' + before)
        reference = expected['source_file_reference']
        binding = (identity, objectid, metadata)
        if reference in identities and identities[reference] != binding:
            raise ValueError('Hardlink group split')
        if identity in owners and owners[identity] != reference:
            raise ValueError('Source groups merged')
        identities[reference] = binding; owners[identity] = reference
    fidelity = all(a['object_id_full64'] == e['source_full64'] for e, a in zip(inputs['rows'], report['rows']))
    if report.get('all_observations_stable') is not True or type(report.get('full_source64_fidelity')) is not bool or report['full_source64_fidelity'] != fidelity:
        raise ValueError('Aggregate observation/fidelity mismatch')
    return {'groups': len(identities), 'paths': len(report['rows']),
            'full64_exact': fidelity,
            'production_executable': False}


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='command', required=True)
    prepare = sub.add_parser('prepare')
    prepare.add_argument('inventory', type=Path)
    prepare.add_argument('bindings', type=Path, help='Externally selected source inventory/WIM/target receipt bindings')
    prepare.add_argument('output', type=Path)
    check = sub.add_parser('check')
    check.add_argument('input', type=Path); check.add_argument('report', type=Path)
    args = parser.parse_args()
    if args.command == 'prepare':
        raw = args.inventory.read_bytes(); bindings = json.loads(args.bindings.read_bytes())
        if hashlib.sha256(raw).hexdigest() != bindings['source_inventory_sha256']:
            raise ValueError('Source inventory hash mismatch')
        for key in ('source_inventory_sha256', 'capture_wim_sha256', 'target_snapshot_sha256', 'target_context_receipt_sha256', 'observer_helper_sha256'):
            exact_hex(bindings[key], 32)
        exact_hex(bindings['target_volume_object_id'], 16)
        if not 0 < bindings['target_volume_serial'] < 2**64:
            raise ValueError('Target serial')
        result = dict(bindings, schema=1, mode='GET-only-installed-objectids', windows_system_drive='C:',
                      rows=source_rows(json.loads(raw)))
        with args.output.open('x') as stream:
            json.dump(result, stream, indent=2); stream.write('\n')
        print(hashlib.sha256(args.output.read_bytes()).hexdigest())
    else:
        raw = args.input.read_bytes(); report = json.loads(args.report.read_bytes())
        if report['input_sha256'] != hashlib.sha256(raw).hexdigest():
            raise ValueError('Observer input hash mismatch')
        print(json.dumps(validate_report(json.loads(raw), report), indent=2))


if __name__ == '__main__':
    main()
