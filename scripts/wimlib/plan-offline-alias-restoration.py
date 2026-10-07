#!/usr/bin/env python3
"""Read-only, fail-closed alias transaction candidate; never invokes a setter."""
import argparse
import collections
import hashlib
import json
import struct
from pathlib import Path


def validate_path(path):
    raw = bytes.fromhex(path)
    if len(raw) % 2 or len(raw) > 65536:
        raise ValueError('invalid UTF16 path bounds')
    text = raw.decode('utf-16-le', 'surrogatepass')
    if not text.startswith('/') or any(unit in text for unit in ('\x00', ':', '\\')) or any(not part or part in ('.', '..') or len(part.encode('utf-16-le', 'surrogatepass')) > 510 for part in text[1:].split('/')):
        raise ValueError('invalid canonical UTF16 path')
    return text


def bounded_read(path, limit=512 * 1024 * 1024):
    with path.open('rb') as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError('input exceeds diagnostic bound')
    return data


def index(rows):
    if len(rows) > 1000000:
        raise ValueError('raw INDEX rows exceed diagnostic bound')
    long, dos, aliases = {}, {}, collections.defaultdict(list)
    for row in rows:
        if row['reference'] != row['actual_reference']:
            raise ValueError('raw INDEX full reference mismatch')
        if row['name'].encode('utf-16-le', 'surrogatepass').hex() != row['name_utf16_le']:
            raise ValueError('raw INDEX UTF16 name mismatch')
        if any(unit in row['name'] for unit in ('/', '\\', '\x00')):
            raise ValueError('invalid raw INDEX leaf name')
        parent = row['parent']
        validate_path((parent + '/' + row['name']).encode('utf-16-le', 'surrogatepass').hex())
        if row['namespace'] == 'Dos':
            key = (parent, row['name_utf16_le'])
            if key in dos:
                raise ValueError('duplicate DOS selector')
            dos[key] = row
            aliases[row['reference']].append(row)
        else:
            key = (parent + '/' + row['name']).encode('utf-16-le', 'surrogatepass').hex()
            if key in long:
                raise ValueError('duplicate long selector')
            long[key] = row
    return long, dos, aliases


def upcase_table(raw):
    if len(raw) != 131072:
        raise ValueError('NTFS upcase table must contain 65536 UTF16 units')
    table = struct.unpack('<65536H', raw)
    if any(table[table[i]] != table[i] for i in range(65536)):
        raise ValueError('NTFS upcase table is not idempotent')
    return table


def folded_name(raw_hex, table):
    raw = bytes.fromhex(raw_hex)
    if len(raw) % 2:
        raise ValueError('odd UTF16 name length')
    if table is None:
        return raw_hex
    return b''.join(struct.pack('<H', table[unit[0]]) for unit in struct.iter_unpack('<H', raw)).hex()


def plan(source_rows, target_rows, desired, posix, upcase=None):
    source_long, source_dos, _ = index(source_rows)
    target_long, target_dos, target_aliases = index(target_rows)
    table = None if upcase is None else upcase_table(upcase)
    folded_dos = {}
    for (parent, name), row in target_dos.items():
        key = (parent, folded_name(name, table))
        if key in folded_dos:
            raise ValueError('target DOS selectors collide under NTFS upcase')
        folded_dos[key] = row
    all_target_names = collections.defaultdict(list)
    for row in target_rows:
        all_target_names[(row['parent'], folded_name(row['name_utf16_le'], table))].append(row)
    requirements, source_refs, seen_paths = {}, set(), set()
    blocked, changes, exact = [], [], 0
    desired_names = {}
    for row in desired:
        path = row['path_utf16_le']
        validate_path(path)
        if path in seen_paths:
            raise ValueError('duplicate source desired path')
        seen_paths.add(path)
        source = source_long.get(path)
        alias = None if source is None else source_dos.get((source['parent'], row['alias_utf16_le']))
        if source is None or source['namespace'] != 'Win32' or alias is None or alias['reference'] != source['reference'] or source['reference'] != row['source_file_id']:
            raise ValueError('source desired DOS lacks exact Win32/INDEX ownership')
        if source['reference'] in source_refs:
            raise ValueError('multiple desired DOS links for source file')
        source_refs.add(source['reference'])
        target = target_long.get(path)
        if target is None:
            blocked.append({'kind': 'missing_target_long_path', 'source': row})
            continue
        if target['namespace'] != 'Win32':
            blocked.append({'kind': 'target_desired_namespace_changed', 'source': row, 'target': target})
        ref = target['reference']
        if ref in requirements:
            raise ValueError('multiple desired DOS links for target file')
        request = {'path_utf16_le': path, 'parent': target['parent'], 'target_reference': ref, 'alias_utf16_le': row['alias_utf16_le'], 'target_long': target}
        alias_key = (target['parent'], folded_name(row['alias_utf16_le'], table))
        if alias_key in desired_names and desired_names[alias_key] != ref:
            raise ValueError('required aliases collide under NTFS upcase')
        desired_names[alias_key] = ref
        requirements[ref] = request
        for named in all_target_names.get(alias_key, []):
            if named['namespace'] != 'Dos' and named['reference'] != ref:
                blocked.append({'kind': 'required_alias_collides_with_long_or_Win32AndDos_name', 'required': request, 'occupant': named})
        occupant = folded_dos.get(alias_key)
        if occupant and occupant['reference'] == ref and occupant['name_utf16_le'] == row['alias_utf16_le']:
            exact += 1
        else:
            changes.append(request)
    for row in posix:
        source = source_long.get(row['path_utf16_le'])
        if source is None or source['namespace'] != 'Posix':
            raise ValueError('POSIX challenge not backed by source namespace')
        target = target_long.get(row['path_utf16_le'])
        if target is None or target['namespace'] != 'Posix':
            blocked.append({'kind': 'POSIX_namespace_or_path_changed', 'source': row, 'target': target})
    affected = {row['target_reference'] for row in changes}
    pending = list(affected)
    while pending:
        ref = pending.pop()
        request = requirements[ref]
        occupant = folded_dos.get((request['parent'], folded_name(request['alias_utf16_le'], table)))
        if occupant and occupant['reference'] not in affected:
            occupied_ref = occupant['reference']
            if occupied_ref not in requirements:
                blocked.append({'kind': 'unknown_occupant_without_source_DOS_requirement', 'required': request, 'occupant': occupant})
            else:
                affected.add(occupied_ref)
                pending.append(occupied_ref)
    clear, assign = [], []
    for ref in sorted(affected):
        request = requirements[ref]
        actual = target_aliases.get(ref, [])
        if len(actual) > 1:
            blocked.append({'kind': 'multiple_target_DOS_links_unsupported', 'required': request, 'actual': actual})
        for alias in actual:
            if alias['parent'] != request['parent']:
                blocked.append({'kind': 'existing_DOS_in_other_parent', 'required': request, 'actual': alias})
            clear.append({'target_reference': ref, 'open_long_path_utf16_le': request['path_utf16_le'], 'expected_old_DOS': alias, 'requested_alias_utf16_le': ''})
        assign.append({'target_reference': ref, 'open_long_path_utf16_le': request['path_utf16_le'], 'required_alias_utf16_le': request['alias_utf16_le'], 'required_parent': request['parent'], 'required_namespace': 'Win32'})
    return {'schema': 1, 'executable': False, 'source_desired': len(desired), 'exact_existing_bindings': exact, 'initial_changed_existing_paths': len(changes), 'affected_target_files': len(affected), 'blocking_counts': dict(collections.Counter(row['kind'] for row in blocked)), 'blocking_conditions': blocked, 'alias_mapping_plan_complete': not blocked, 'source_target_alias_plan_complete': False, 'protocol_blockers': ['actual_target_NTFS_upcase_collision_proof_missing', 'complete_affected_metadata_invariants_missing', 'independent_expected_input_hash_binding_missing'], 'phases': [{'phase': 'clear_all_affected_existing_DOS_before_any_assignment', 'operations': clear}, {'phase': 'assign_exact_source_alias_to_selected_Win32_long_link', 'operations': assign}], 'required_before_execution': ['Resolve every missing source path, changed namespace, unknown occupant and unsupported target alias state; no waiver', 'Disposable target child only, immutable source/baseline hashes unchanged', 'Record full rawSD, all timestamps including change time, payload/ADS hashes, EAs, reparses, storage attributes, hardlink paths/full IDs for every affected file and parent', 'Close/reopen and independently verify complete raw INDEX long/DOS full-reference ownership and all metadata invariants', 'No timestamp/security restoration capability or all-or-nothing execution claimed by this read-only planner'], 'policy': 'Candidate clear-then-assign closure includes all required occupants, not just wrong-edge cycles. No OS API calls, input mutation, namespace fallback or executable approval.'}


def verify_expected_hashes(receipts, expected):
    selected = {}
    for specification in expected:
        name, separator, digest = specification.partition('=')
        if not separator or name not in receipts or name in selected or len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
            raise ValueError('invalid externally selected input hash')
        if receipts[name]['sha256'] != digest:
            raise ValueError('externally selected input hash mismatch')
        selected[name] = digest
    return set(selected) == set(receipts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('source_index', 'target_index', 'desired', 'posix', 'output'):
        parser.add_argument(name, type=Path)
    parser.add_argument('--invariant-input', action='append', type=Path, default=[], help='Hash-bind preserved read-only metadata evidence; this does not prove complete execution invariants')
    parser.add_argument('--expected-input-sha256', action='append', default=[], metavar='INPUT_NAME=SHA256', help='Externally selected expected hash; mismatches rejected before planning')
    parser.add_argument('--target-upcase', type=Path)
    parser.add_argument('--expected-target-upcase-sha256')
    args = parser.parse_args()
    if bool(args.target_upcase) != bool(args.expected_target_upcase_sha256):
        raise ValueError('upcase path and external expected hash must be supplied together')
    upcase = None
    if args.target_upcase:
        upcase = bounded_read(args.target_upcase, 131072)
        if hashlib.sha256(upcase).hexdigest() != args.expected_target_upcase_sha256:
            raise ValueError('target upcase expected hash mismatch')
    parsed, receipts = {}, {}
    for name in ('source_index', 'target_index', 'desired', 'posix'):
        path = getattr(args, name)
        data = bounded_read(path)
        parsed[name] = json.loads(data)
        receipts[name] = {'path': str(path), 'sha256': hashlib.sha256(data).hexdigest()}
    external_binding = verify_expected_hashes(receipts, args.expected_input_sha256)
    result = plan(parsed['source_index']['raw_directory_index_entries'], parsed['target_index']['raw_directory_index_entries'], parsed['desired'], parsed['posix'], upcase)
    result['inputs'] = receipts
    result['target_ntfs_upcase_applied'] = upcase is not None
    if upcase is not None:
        result['target_upcase'] = {'path': str(args.target_upcase), 'sha256': hashlib.sha256(upcase).hexdigest()}
        result['protocol_blockers'].remove('actual_target_NTFS_upcase_collision_proof_missing')
    result['externally_selected_expected_hashes'] = args.expected_input_sha256
    result['external_expected_input_binding_complete'] = external_binding
    if external_binding:
        result['protocol_blockers'].remove('independent_expected_input_hash_binding_missing')
    result['preserved_invariant_inputs'] = []
    for path in args.invariant_input:
        data = bounded_read(path)
        result['preserved_invariant_inputs'].append({'path': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
    result['all_affected_metadata_invariants_captured'] = False
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
    print(json.dumps({key: result[key] for key in ('executable', 'source_desired', 'affected_target_files', 'blocking_counts', 'source_target_alias_plan_complete')}))


if __name__ == '__main__':
    main()
