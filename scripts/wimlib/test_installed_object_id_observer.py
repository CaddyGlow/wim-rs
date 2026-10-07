import copy
import importlib.util
import unittest
from pathlib import Path
spec = importlib.util.spec_from_file_location('observer', Path(__file__).with_name('check-installed-object-id-observer.py'))
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)

class InstalledObserver(unittest.TestCase):
    def fixture(self):
        metadata = dict(attributes=32, creation_time=1, access_time=2, write_time=3, change_time=4, security_raw='01000080'+'00'*16)
        expected = dict(path_utf16_le='/dir/file'.encode('utf-16le').hex(), source_full64='01'*16+'02'*48, source_file_reference=123)
        bindings = dict(source_inventory_sha256='03'*32, capture_wim_sha256='04'*32, target_snapshot_sha256='05'*32, target_context_receipt_sha256='06'*32,observer_helper_sha256='08'*32)
        inputs = dict(bindings, target_volume_object_id='09'*16, target_volume_serial=9, rows=[expected])
        identity = (9).to_bytes(8,'little').hex()+'07'*16
        actual = dict(expected, error=None, identity_observation='WindowsFileIdInfo', file_id_info24=identity, first_file_id_info24=identity, object_id_full64=expected['source_full64'], first_object_id_full64=expected['source_full64'], metadata=metadata, first_metadata=metadata, first_after_get_metadata=metadata, reopened_after_get_metadata=metadata)
        report = dict(bindings, target_volume_object_id_bound_context='09'*16,target_volume_object_id_observed_by_this_helper=False, mode='GET-only-installed-objectids', production_executable=False,all_observations_stable=True,full_source64_fidelity=True, rows=[actual])
        return inputs, report

    def test_exact_observations_pass_but_are_not_executable(self):
        i,r=self.fixture(); self.assertEqual(m.validate_report(i,r),dict(groups=1,paths=1,full64_exact=True,production_executable=False))

    def test_zero_extended_fields_classified_without_repair(self):
        i,r=self.fixture();r['rows'][0]['object_id_full64']='01'*16+'00'*48;r['rows'][0]['first_object_id_full64']=r['rows'][0]['object_id_full64'];r['full_source64_fidelity']=False;self.assertFalse(m.validate_report(i,r)['full64_exact'])

    def test_identity_change_and_metadata_side_effect_rejected(self):
        for field in ('first_file_id_info24','first_object_id_full64'):
            i,r=self.fixture();r['rows'][0][field]='ff'* (24 if field.endswith('24') else 64)
            with self.assertRaises(ValueError):m.validate_report(i,r)
        i,r=self.fixture();r['rows'][0]['first_metadata']=dict(r['rows'][0]['metadata'],access_time=99)
        with self.assertRaisesRegex(ValueError,'GET changed'):m.validate_report(i,r)

    def test_typed_metadata_and_security_header_rejected(self):
        for key, value in (('access_time', True), ('change_time', -1), ('attributes', 2**32), ('security_raw', '01'*20)):
            i,r=self.fixture(); r['rows'][0]['metadata']=dict(r['rows'][0]['metadata'], **{key:value})
            with self.assertRaises(ValueError):m.validate_report(i,r)

    def test_false_source_binding_and_aggregates_rejected(self):
        for key,value in (('source_file_reference',456), ('source_full64','aa'*64)):
            i,r=self.fixture();r['rows'][0][key]=value
            with self.assertRaisesRegex(ValueError,'Source row'):m.validate_report(i,r)
        for key in ('all_observations_stable','full_source64_fidelity'):
            i,r=self.fixture();r[key]=False
            with self.assertRaisesRegex(ValueError,'Aggregate'):m.validate_report(i,r)

    def test_hardlink_split_rejected(self):
        i,r=self.fixture();i['rows'].append(dict(i['rows'][0],path_utf16_le='/dir/alias'.encode('utf-16le').hex()));a=copy.deepcopy(r['rows'][0]);a['path_utf16_le']=i['rows'][1]['path_utf16_le'];a['file_id_info24']=a['first_file_id_info24']=(9).to_bytes(8,'little').hex()+'08'*16;r['rows'].append(a)
        with self.assertRaisesRegex(ValueError,'split'):m.validate_report(i,r)

    def test_escape_and_duplicate_source_selectors_rejected(self):
        for path in ('/../x','/./x','/x//y','/x\\y','/x:y','C:/x'):
            with self.assertRaises(ValueError):m.selector(path.encode('utf-16le').hex())
        i,r=self.fixture();g=dict(file_reference=1,object_id_full64='01'*64,paths=[{'path_utf16_le':i['rows'][0]['path_utf16_le']}]*2)
        with self.assertRaisesRegex(ValueError,'Duplicate'):m.source_rows(dict(groups=[g],object_id_paths=2,object_id_file_groups=1))

    def test_changed_provenance_and_missing_rows_rejected(self):
        i,r=self.fixture();r['target_snapshot_sha256']='aa'*32
        with self.assertRaisesRegex(ValueError,'Provenance'):m.validate_report(i,r)
        i,r=self.fixture();r['rows']=[]
        with self.assertRaisesRegex(ValueError,'Missing'):m.validate_report(i,r)

if __name__=='__main__':unittest.main()
