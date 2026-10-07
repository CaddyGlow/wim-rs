"""Fresh capture must let Setup choose the destination recovery partition."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location('qcow2_wim_oracle', HERE / 'check-qcow2-wim-oracle.py')
ORACLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ORACLE)
ROOT = HERE.parents[2] / 'windows-uup'


class RecoveryConfigurationTests(unittest.TestCase):
    def setUp(self):
        source = (ROOT / 'crates/windows-uup/src/capture_disk.rs').read_text()
        self.xml = source.split('const STAGED_REAGENT_XML: &str = r#"', 1)[1].split('"#;', 1)[0]
        self.native = ROOT / 'crates/windows-uup/tests/fixtures/capture/source-native-reagent.xml'

    def check_xml(self, xml):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'ReAgent.xml'
            path.write_text(xml)
            return ORACLE.recovery_configuration(path, self.native)

    def test_unbound_capture_configuration_allows_recovery_partition_placement(self):
        self.assertTrue(self.check_xml(self.xml)['passes'])

    def test_staged_os_placement_policy_is_rejected(self):
        report = self.check_xml(self.xml.replace('<WinREStaged state="0"/>', '<WinREStaged state="1"/>'))
        self.assertEqual(report['errors'], ['WinREStaged: fresh-install state or binding differs'])

    def test_source_disk_binding_is_rejected(self):
        xml = self.xml.replace('offset="0"', 'offset="1048576"', 1)
        self.assertIn('WinreLocation: fresh-install state or binding differs', self.check_xml(xml)['errors'])


if __name__ == '__main__':
    unittest.main()
