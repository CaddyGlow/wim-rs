"""Host regressions for evidence validation; no guest operation is performed."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
SPEC = importlib.util.spec_from_file_location('installation', HERE / 'check-qcow2-installation.py')
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class EvidenceTests(unittest.TestCase):
    def test_firmware_identity_uses_native_api_and_propagates_failure(self):
        self.assertIn('GetFirmwareType(out uint type)', gate.IDENTITY)
        self.assertIn('if(!GetFirmwareType(out type))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error())', gate.IDENTITY)
        self.assertIn('FirmwareType=[FirmwareProbe]::Type()', gate.IDENTITY)
        self.assertNotIn('PEFirmwareType', gate.IDENTITY)

    def test_runtime_inventory_queries_reparse_metadata_without_following_targets(self):
        command = gate.metadata_command()
        self.assertIn('CreateFile(p,write?0xC0000000u:0x80000000u,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)', command)
        self.assertIn('EA=[NtfsProbe]::EA($p);ObjectID=[NtfsProbe]::ObjectID($p,$false)', command)
        self.assertNotIn('if(($i.Attributes-band 1024)-eq 0)', command)
        self.assertIn('IntPtr.Zero,3,0x02000000,IntPtr.Zero)', gate.NTFS['HELPER'])
        reparse = {'Info': {'Attributes': 1024}, 'EA': '', 'ObjectID': ''}
        self.assertTrue(gate.complete_ea_object_id_baseline([reparse]))
        self.assertFalse(gate.complete_ea_object_id_baseline([{'Info': reparse['Info']}]))

    def test_object_id_baseline_requires_complete_buffer_and_ea_evidence(self):
        row = {'Info': {'Attributes': 32}, 'EA': '', 'ObjectID': '-'.join(['01'] * 64)}
        self.assertTrue(gate.complete_ea_object_id_baseline([row]))
        self.assertTrue(gate.complete_ea_object_id_baseline([dict(row, ObjectID='')]))
        for value in ('01', '-'.join(['01'] * 16), '-'.join(['01'] * 63), 'GG'):
            self.assertFalse(gate.complete_ea_object_id_baseline([dict(row, ObjectID=value)]))
        self.assertFalse(gate.complete_ea_object_id_baseline([{'Info': row['Info'], 'ObjectID': row['ObjectID']}]))
        self.assertFalse(gate.complete_ea_object_id_baseline([dict(row, EA='invalid')]))
        self.assertFalse(gate.complete_ea_object_id_baseline([]))

    def test_extended_object_id_loss_fails_metadata_comparison(self):
        row = {'Path': r'\NativeDiskCapture\payload.bin',
               'Info': {'Identity': 'source-id', 'Attributes': 32},
               'Streams': [], 'Short': '', 'EA': '',
               'ObjectID': '-'.join(['01'] * 64)}
        target = dict(row, ObjectID='-'.join(['01'] * 16 + ['00'] * 48))
        target['Info'] = dict(row['Info'], Identity='new-target-id')
        self.assertEqual(gate.metadata_differences([row], [row]), [])
        differences = gate.metadata_differences([row], [target])
        self.assertEqual(len(differences), 1)
        self.assertNotEqual(differences[0]['source']['ObjectID'],
                            differences[0]['installed']['ObjectID'])

    def test_oobe_requires_provisioned_account_and_matching_credentials(self):
        answer = HERE / 'qcow2-install-autounattend.xml'
        gate.validate_installation_account(answer, 'capturegate')
        with self.assertRaises(ValueError):
            gate.validate_installation_account(answer, 'deploy')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'answer.xml'
            tree = gate.ET.parse(answer)
            ns = {'u': 'urn:schemas-microsoft-com:unattend'}
            account = tree.find("u:settings[@pass='oobeSystem']/u:component/u:UserAccounts", ns)
            parent = tree.find("u:settings[@pass='oobeSystem']/u:component[@name='Microsoft-Windows-Shell-Setup']", ns)
            parent.remove(account)
            tree.write(path)
            with self.assertRaises(ValueError):
                gate.validate_installation_account(path, 'capturegate')
            tree = gate.ET.parse(answer)
            tree.find("u:settings[@pass='oobeSystem']/u:component/u:AutoLogon/u:Password/u:Value", ns).text = 'mismatched'
            tree.write(path)
            with self.assertRaises(ValueError):
                gate.validate_installation_account(path, 'capturegate')

    def recovery(self, kind='os'):
        source = {'Disks': [{'Guid': '{source-disk}'}], 'Partitions': [{'Guid': '{source-partition}'}]}
        identity = {'RecoveryGuid': '{destination-disk}', 'RecoveryOffset': '1048576',
                    'RecoveryLocation': r'\\?\GLOBALROOT\device\harddisk0\partition3\Recovery\WindowsRE',
                    'RecoveryRelativePath': r'\Recovery\WindowsRE',
                    'Disks': [{'Guid': '{destination-disk}', 'Number': 0, 'PartitionStyle': 'GPT'}],
                    'Partitions': [{'Guid': '{destination-partition}', 'Offset': 1048576,
                                    'Number': 3, 'Letter': 'C',
                                    'GptType': '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}' if kind == 'os'
                                    else '{de94bba4-06d1-4d40-a16a-bfd50179d6ac}'}]}
        return source, identity

    def test_new_os_and_recovery_partition_bindings_are_valid(self):
        for kind in ('os', 'recovery'):
            source, identity = self.recovery(kind)
            self.assertTrue(gate.recovery_destination(identity, source)['passes'])

    def test_os_registration_does_not_pass_separate_recovery_profile(self):
        for kind in ('os', 'recovery'):
            source, identity = self.recovery(kind)
            result = gate.recovery_destination(identity, source)
            self.assertTrue(result['passes'])
            self.assertEqual(result['separate_recovery_partition_passes'], kind == 'recovery')

    def test_recovery_rejects_historical_pass_without_separate_partition_gate(self):
        for gates in ({}, {'winre_separate_recovery_partition': False}):
            state = {'steps': {'observation': {'passes': True, 'gates': gates}}}
            with self.assertRaisesRegex(RuntimeError, 'separate recovery-partition'):
                gate.recovery(None, state)

    def test_source_binding_wrong_offset_or_traversal_are_rejected(self):
        source, identity = self.recovery()
        for field, value in [('RecoveryGuid', '{source-disk}'), ('RecoveryOffset', '0'),
                             ('RecoveryRelativePath', r'\Recovery\..\WindowsRE')]:
            altered = dict(identity, **{field: value})
            self.assertFalse(gate.recovery_destination(altered, source)['passes'])

    def test_boolean_only_equivalence_cannot_waive_container_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'proof.json'
            path.write_text(json.dumps({'schema': 1, 'kind': 'winre-full-equivalence', 'passes': True,
                                        'source_sha256': 'a' * 64, 'target_sha256': 'b' * 64,
                                        'entry_count': 1, 'stream_count': 1,
                                        'differences': [], 'artifacts': {}}))
            result = gate.winre_equivalence(path, 'a' * 64, 'b' * 64)
            self.assertFalse(result['passes'])
            self.assertIn('missing', result['errors'][0])

    def test_equivalence_for_another_container_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'proof.json'
            path.write_text(json.dumps({'schema': 1, 'kind': 'winre-full-equivalence', 'passes': True,
                                        'source_sha256': 'a' * 64, 'target_sha256': 'b' * 64}))
            self.assertFalse(gate.winre_equivalence(path, 'a' * 64, 'c' * 64)['passes'])


if __name__ == '__main__':
    unittest.main()
