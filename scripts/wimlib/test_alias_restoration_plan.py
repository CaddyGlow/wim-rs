#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest
import tempfile
import struct

spec = importlib.util.spec_from_file_location('planner', Path(__file__).with_name('plan-offline-alias-restoration.py'))
planner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(planner)


def entry(name, ref, namespace='Win32', parent='/P'):
    return {'parent': parent, 'name': name, 'name_utf16_le': name.encode('utf-16-le', 'surrogatepass').hex(), 'namespace': namespace, 'reference': ref, 'actual_reference': ref}


def desired(name, alias, ref):
    return {'path_utf16_le': ('/P/' + name).encode('utf-16-le', 'surrogatepass').hex(), 'alias_utf16_le': alias.encode('utf-16-le', 'surrogatepass').hex(), 'source_file_id': ref}


def fixture(names):
    source, requests = [], []
    for i, name in enumerate(names, 1):
        alias = 'FILE~%d' % i
        source.extend([entry(name, i), entry(alias, i, 'Dos')])
        requests.append(desired(name, alias, i))
    return source, requests


class PlanTests(unittest.TestCase):
    def test_windows_stream_and_drive_escape_paths_are_rejected(self):
        for text in ('/P/file:stream', '/P/C:escape', '/P/back\\slash'):
            with self.assertRaises(ValueError):
                planner.validate_path(text.encode('utf-16-le').hex())

    def test_bounded_read_rejects_extra_byte_and_accepts_exact_limit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'input'
            path.write_bytes(b'12345678')
            self.assertEqual(planner.bounded_read(path, 8), b'12345678')
            path.write_bytes(b'123456789')
            with self.assertRaises(ValueError):
                planner.bounded_read(path, 8)

    def ascii_upcase(self):
        return struct.pack('<65536H', *(i - 32 if 97 <= i <= 122 else i for i in range(65536)))

    def test_case_insensitive_long_name_collision_is_blocked(self):
        source, requests = fixture(['Alpha'])
        target = [entry('Alpha', 11), entry('file~1', 99, 'Win32AndDos')]
        result = planner.plan(source, target, requests, [], self.ascii_upcase())
        self.assertEqual(result['blocking_counts']['required_alias_collides_with_long_or_Win32AndDos_name'], 1)

    def test_case_only_alias_difference_still_requires_exact_assignment(self):
        source, requests = fixture(['Alpha'])
        target = [entry('Alpha', 11), entry('file~1', 11, 'Dos')]
        result = planner.plan(source, target, requests, [], self.ascii_upcase())
        self.assertEqual(result['exact_existing_bindings'], 0)
        self.assertEqual(result['affected_target_files'], 1)

    def test_short_or_non_idempotent_upcase_table_is_rejected(self):
        with self.assertRaises(ValueError):
            planner.upcase_table(b'')
        raw = bytearray(self.ascii_upcase())
        struct.pack_into('<H', raw, 2 * 65, 66)
        with self.assertRaises(ValueError):
            planner.upcase_table(raw)

    def test_cycle_clears_every_alias_before_assignments(self):
        source, requests = fixture(['Alpha', 'Bravo'])
        target = [entry('Alpha', 11), entry('Bravo', 12), entry('FILE~2', 11, 'Dos'), entry('FILE~1', 12, 'Dos')]
        result = planner.plan(source, target, requests, [])
        self.assertFalse(result['executable'])
        self.assertTrue(result['alias_mapping_plan_complete'])
        self.assertEqual(result['affected_target_files'], 2)
        self.assertEqual([len(p['operations']) for p in result['phases']], [2, 2])
        self.assertTrue(all(x['requested_alias_utf16_le'] == '' for x in result['phases'][0]['operations']))

    def test_chain_includes_required_occupant_without_existing_alias(self):
        source, requests = fixture(['Alpha', 'Bravo', 'Charlie'])
        target = [entry('Alpha', 11), entry('Bravo', 12), entry('Charlie', 13), entry('FILE~1', 12, 'Dos'), entry('FILE~2', 13, 'Dos')]
        result = planner.plan(source, target, requests, [])
        self.assertTrue(result['alias_mapping_plan_complete'])
        self.assertEqual(result['affected_target_files'], 3)
        self.assertEqual([len(p['operations']) for p in result['phases']], [2, 3])

    def test_unknown_occupant_blocks_complete_plan(self):
        source, requests = fixture(['Alpha'])
        target = [entry('Alpha', 11), entry('Unplanned', 99), entry('FILE~1', 99, 'Dos')]
        result = planner.plan(source, target, requests, [])
        self.assertFalse(result['alias_mapping_plan_complete'])
        self.assertEqual(result['blocking_counts']['unknown_occupant_without_source_DOS_requirement'], 1)
        self.assertFalse(result['executable'])

    def test_posix_does_not_inherit_same_parent_inode_alias(self):
        source, requests = fixture(['Alpha'])
        source.append(entry('PosixLink', 1, 'Posix'))
        target = [entry('Alpha', 11), entry('FILE~1', 11, 'Dos'), entry('PosixLink', 11, 'Posix')]
        posix = [desired('PosixLink', 'FILE~1', 1)]
        result = planner.plan(source, target, requests, posix)
        self.assertTrue(result['alias_mapping_plan_complete'])
        self.assertEqual(result['affected_target_files'], 0)

    def test_missing_long_and_changed_posix_namespace_remain_blockers(self):
        source, requests = fixture(['Alpha'])
        source.append(entry('PosixLink', 1, 'Posix'))
        result = planner.plan(source, [entry('PosixLink', 11)], requests, [desired('PosixLink', 'FILE~1', 1)])
        self.assertEqual(result['blocking_counts'], {'missing_target_long_path': 1, 'POSIX_namespace_or_path_changed': 1})

    def test_existing_alias_in_other_parent_and_posix_selection_blocked(self):
        source, requests = fixture(['Alpha'])
        target = [entry('Alpha', 11, 'Posix'), entry('OtherLong', 11, parent='/Q'), entry('OTHER~1', 11, 'Dos', '/Q')]
        result = planner.plan(source, target, requests, [])
        self.assertEqual(result['blocking_counts']['target_desired_namespace_changed'], 1)
        self.assertEqual(result['blocking_counts']['existing_DOS_in_other_parent'], 1)
        self.assertFalse(result['alias_mapping_plan_complete'])

    def test_multiple_aliases_for_target_file_never_narrowed(self):
        source, requests = fixture(['Alpha'])
        target = [entry('Alpha', 11), entry('OTHER~1', 11, 'Dos'), entry('OTHER~2', 11, 'Dos', '/Q')]
        result = planner.plan(source, target, requests, [])
        self.assertEqual(result['blocking_counts']['multiple_target_DOS_links_unsupported'], 1)
        self.assertEqual(len(result['phases'][0]['operations']), 2)
        self.assertFalse(result['executable'])

    def test_combined_namespace_and_longname_occupants_cannot_be_cleared(self):
        source, requests = fixture(['Alpha'])
        for namespace in ('Win32AndDos', 'Win32', 'Posix'):
            target = [entry('Alpha', 11), entry('FILE~1', 99, namespace)]
            result = planner.plan(source, target, requests, [])
            self.assertEqual(result['blocking_counts']['required_alias_collides_with_long_or_Win32AndDos_name'], 1)
            self.assertFalse(result['alias_mapping_plan_complete'])
            self.assertEqual(result['phases'][0]['operations'], [])

    def test_surrogate_name_preserved_and_invalid_path_rejected(self):
        name = 'Alpha\ud800'
        source, requests = fixture(['Alpha'])
        source, requests = fixture([name])
        result = planner.plan(source, [entry(name, 11)], requests, [])
        self.assertEqual(result['phases'][1]['operations'][0]['open_long_path_utf16_le'], requests[0]['path_utf16_le'])
        self.assertEqual(planner.validate_path(('/P/' + name).encode('utf-16-le', 'surrogatepass').hex()), '/P/' + name)
        for path in ('P/Alpha', '/P//Alpha', '/P/Al\x00pha'):
            with self.assertRaisesRegex(ValueError, 'canonical'):
                planner.validate_path(path.encode('utf-16-le', 'surrogatepass').hex())

    def test_unbound_upcase_proof_never_marks_plan_complete(self):
        source, requests = fixture(['Alpha'])
        result = planner.plan(source, [entry('Alpha', 11)], requests, [])
        self.assertTrue(result['alias_mapping_plan_complete'])
        self.assertFalse(result['source_target_alias_plan_complete'])
        self.assertIn('actual_target_NTFS_upcase_collision_proof_missing', result['protocol_blockers'])

    def test_external_expected_hash_mismatch_rejected(self):
        receipts = {'source_index': {'sha256': 'a' * 64}}
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            planner.verify_expected_hashes(receipts, ['source_index=' + 'b' * 64])
        with self.assertRaisesRegex(ValueError, 'invalid externally'):
            planner.verify_expected_hashes(receipts, ['unknown=' + 'a' * 64])

    def test_missing_expected_hash_does_not_claim_external_binding(self):
        receipts = {'source_index': {'sha256': 'a' * 64}, 'target_index': {'sha256': 'b' * 64}}
        self.assertFalse(planner.verify_expected_hashes(receipts, ['source_index=' + 'a' * 64]))
        self.assertTrue(planner.verify_expected_hashes(receipts, ['source_index=' + 'a' * 64, 'target_index=' + 'b' * 64]))

    def test_raw_leaf_separator_is_not_treated_as_nested_path(self):
        source, requests = fixture(['Alpha'])
        for name in ('Alpha/Bravo', 'Alpha\\Bravo', 'Alpha\x00Bravo'):
            with self.assertRaisesRegex(ValueError, 'leaf name'):
                planner.plan(source, [entry(name, 11)], requests, [])

    def test_stale_index_reference_rejected(self):
        source, requests = fixture(['Alpha'])
        target = entry('Alpha', 11)
        target['actual_reference'] = 12
        with self.assertRaisesRegex(ValueError, 'full reference'):
            planner.plan(source, [target], requests, [])


if __name__ == '__main__':
    unittest.main()
