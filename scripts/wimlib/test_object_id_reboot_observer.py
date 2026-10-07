import copy
import importlib.util
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('observer',Path(__file__).with_name('check-object-id-reboot-observer.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
EXPECTED={'path_utf16_le':(module.SCRATCH_ROOT+'\\file.bin').encode('utf-16-le').hex(), 'object_id_full64':bytes(range(64)).hex(),'file_id_info24':bytes(range(24)).hex()}
ACTUAL=dict(EXPECTED,error=None,passed=True)

class ObjectIdReadOnlyPersistence(unittest.TestCase):
    def test_exact_full64_and_file_identity_pass(self):
        module.validate_row(EXPECTED,ACTUAL)

    def test_extended_loss_fails_with_first16_unchanged(self):
        actual=copy.deepcopy(ACTUAL);actual['object_id_full64']=bytes(range(16)).hex()+'00'*48
        with self.assertRaisesRegex(ValueError,'object_id_full64'):module.validate_row(EXPECTED,actual)

    def test_file_replacement_identity_fails_even_with_same_object_id(self):
        actual=copy.deepcopy(ACTUAL);actual['file_id_info24']='ff'+actual['file_id_info24'][2:]
        with self.assertRaisesRegex(ValueError,'file_id_info24'):module.validate_row(EXPECTED,actual)

    def test_missing_object_id_fails(self):
        actual=copy.deepcopy(ACTUAL);actual['error']='Object identifier not found';actual['object_id_full64']=None
        with self.assertRaises(ValueError):module.validate_row(EXPECTED,actual)

    def test_partial16_byte_object_id_fails(self):
        actual=copy.deepcopy(ACTUAL);actual['object_id_full64']=actual['object_id_full64'][:32]
        with self.assertRaises(ValueError):module.validate_row(EXPECTED,actual)

    def test_scratch_prefix_does_not_allow_escape_or_other_drive(self):
        for path in (module.SCRATCH_ROOT+r'\..\Windows\file.bin', module.SCRATCH_ROOT+r'\.\file.bin', module.SCRATCH_ROOT+'/file.bin', 'C:'+module.SCRATCH_ROOT[2:]+r'\file.bin', r'T:\different-scratch\file.bin'):
            with self.assertRaises(ValueError):
                module.validate_selector(path.encode('utf-16-le').hex())

    def test_changed_raw_selector_fails(self):
        actual=copy.deepcopy(ACTUAL);actual['path_utf16_le']='54003a005c006700'
        with self.assertRaises(ValueError):module.validate_row(EXPECTED,actual)

if __name__=='__main__':unittest.main()
