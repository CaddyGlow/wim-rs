import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('checker', Path(__file__).with_name('check-dos-alias-scratch.py'))
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class ScratchValidationTests(unittest.TestCase):
    def test_rejects_non_scratch_root(self):
        with self.assertRaises(ValueError):
            checker.validate({'Root': 'C:\\Windows'})

    def test_rejects_empty_api_observations(self):
        with self.assertRaises(ValueError):
            checker.validate({'Root': 'T:\\Temp\\QCOW2-AliasScratch-' + 'a' * 32, 'Actions': []})

    def test_rejects_mutation_outside_new_scratch(self):
        with self.assertRaises(ValueError):
            checker.validate({'Root': 'T:\\Temp\\QCOW2-AliasScratch-' + 'a' * 32,
                              'Actions': [{'Path': 'C:\\Windows\\explorer.exe'}]})

    def test_rejects_changed_security_even_after_successful_api(self):
        root = 'T:\\Temp\\QCOW2-AliasScratch-' + 'a' * 32
        path = root + '\\sample.bin'
        before = {'Path': path, 'Identity24': '00' * 24,
                  'PayloadSHA256': '11' * 32, 'SecurityOwnerGroupDacl': '0100'}
        after = dict(before, SecurityOwnerGroupDacl='0200')
        action = {'Path': path, 'Alias': '', 'AliasUtf16': '', 'Before': before, 'After': after}
        with self.assertRaisesRegex(ValueError, 'SecurityOwnerGroupDacl'):
            checker.validate({'Root': root, 'Actions': [action]})

    def test_rejects_truncated_file_identity(self):
        root = 'T:\\Temp\\QCOW2-AliasScratch-' + 'a' * 32
        path = root + '\\sample.bin'
        observation = {'Path': path, 'Identity24': '00' * 16}
        action = {'Path': path, 'Alias': '', 'AliasUtf16': '',
                  'Before': observation, 'After': observation}
        with self.assertRaisesRegex(ValueError, '24-byte'):
            checker.validate({'Root': root, 'Actions': [action]})

    def test_volume_root_parent_reaches_observation_validation(self):
        with self.assertRaisesRegex(ValueError, 'missing actions'):
            checker.validate({'Root': 'T:\\QCOW2-AliasScratch-' + 'a' * 32, 'Actions': []})

    def test_rejects_parent_traversal(self):
        with self.assertRaisesRegex(ValueError, 'traversal'):
            checker.validate({'Root': 'T:\\..\\QCOW2-AliasScratch-' + 'a' * 32, 'Actions': []})

    def test_rejects_missing_hardlink_ordering_case(self):
        with self.assertRaisesRegex(ValueError, 'ordering cases'):
            checker.validate_ordering([])

    def test_rejects_aliases_remaining_after_clear(self):
        rows = [{'FirstLink': i, 'ClearStatuses': [0, 0, 0],
                 'EmptyParentEnumeration': [{'Alias': 'OLD~1'}] * 3} for i in range(3)]
        with self.assertRaisesRegex(ValueError, 'prior aliases'):
            checker.validate_ordering(rows)


if __name__ == '__main__':
    unittest.main()
