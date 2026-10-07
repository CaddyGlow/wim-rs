import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('installed_aliases', Path(__file__).with_name('check-installed-dos-aliases.py'))
aliases = importlib.util.module_from_spec(spec)
spec.loader.exec_module(aliases)


class InstalledAliasReceipts(unittest.TestCase):
    def setUp(self):
        self.expected = [{'path_utf16_le': '2f004100', 'alias_utf16_le': '41007e003100'}]
        self.row = {'PathUTF16': '2f004100', 'AliasUTF16': '41007e003100',
                    'LongFileID': '01' * 24, 'AliasFileID': '01' * 24, 'Pass': True}

    def report(self, rows):
        return {'InventorySHA256': 'ab', 'Count': len(rows), 'Failures': 0, 'Rows': rows}

    def test_complete_file_id_info_receipt_is_accepted(self):
        self.assertEqual(aliases.validate_report(self.expected, self.report([self.row]), 'ab'), 0)

    def test_truncated_id_cannot_be_credited_as_matching(self):
        row = dict(self.row, LongFileID='01' * 16, AliasFileID='01' * 16)
        with self.assertRaises(ValueError):
            aliases.validate_report(self.expected, self.report([row]), 'ab')

    def test_missing_or_substituted_selector_cannot_be_credited(self):
        with self.assertRaises(ValueError):
            aliases.validate_report(self.expected, self.report([]), 'ab')
        with self.assertRaises(ValueError):
            aliases.validate_report(self.expected, self.report([dict(self.row, AliasUTF16='4200')]), 'ab')

    def test_failure_count_cannot_hide_a_failed_open(self):
        with self.assertRaises(ValueError):
            aliases.validate_report(self.expected, self.report([dict(self.row, Pass=False)]), 'ab')


if __name__ == '__main__':
    unittest.main()
