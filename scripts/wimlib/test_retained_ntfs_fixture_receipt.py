import copy
import importlib.util
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('receipt',Path(__file__).with_name('check-retained-ntfs-fixture-receipt.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

def receipt():
    disks=[{'serial':s,'requested_letter':letter,'number':index,'is_boot':False,'is_system':False,'virtual_bytes':536870912,'before_style':'RAW','after_style':'MBR','after_partitions':[{'DiskNumber':index,'PartitionNumber':1,'Offset':1048576,'Size':535822336,'DriveLetter':letter}],'after_volume':{'FileSystem':'NTFS','DriveLetter':letter,'Size':535818240}} for index,(s,letter) in enumerate([('V12-CASE-FIXTURE','V'),('V12-ADS-FIXTURE','W')],1)]
    return {'completed':True,'disks':disks,'case':{'path':r'V:\case','control_path':r'V:\control','empty':True,'global_policy_changed':False,'control_attributes':16,'enable_exit':0,'query_exit':0,'control_query_exit':0},'ads':{'path':r'W:\OddFixture\many-ads.bin','partition':1,'unnamed_byte':170,'count':160,'verified_streams':[{'name':f'stream-{i:03d}','length':1,'byte':i} for i in range(160)],'windows_stream_enumeration':[{'Stream':f'stream-{i:03d}','Length':1} for i in range(160)]}}

class FixtureReceipt(unittest.TestCase):
    def test_expected_independent_windows_fixture_receipt(self):module.validate(receipt())
    def test_duplicate_serial_or_swapped_letter_rejected(self):
        r=receipt();r['disks'][1]['serial']=r['disks'][0]['serial'];r['disks'][1]['requested_letter']='V'
        with self.assertRaises(ValueError):module.validate(r)
        r=receipt();r['disks'][1]['requested_letter']='V'
        with self.assertRaises(ValueError):module.validate(r)
    def test_formatted_volume_and_partition_binding_required(self):
        for section,key,value in [('after_volume','FileSystem','FAT32'),('after_volume','DriveLetter','X'),('after_volume','Size',536870913)]:
            r=receipt();r['disks'][0][section][key]=value
            with self.assertRaises(ValueError):module.validate(r)
        for key,value in [('DiskNumber',99),('Offset',0),('Size',536870913),('DriveLetter','W')]:
            r=receipt();r['disks'][0]['after_partitions'][0][key]=value
            with self.assertRaises(ValueError):module.validate(r)
        r=receipt();r['disks'][0]['after_style']='GPT'
        with self.assertRaises(ValueError):module.validate(r)
    def test_unnamed_byte_mismatch_rejected(self):
        r=receipt();r['ads']['unnamed_byte']=0
        with self.assertRaises(ValueError):module.validate(r)
    def test_boot_or_reused_disk_rejected(self):
        for key,value in [('is_boot',True),('before_style','GPT'),('number',0)]:
            r=receipt();r['disks'][0][key]=value
            with self.assertRaises(ValueError):module.validate(r)
    def test_missing_or_wrong_stream159_rejected(self):
        r=receipt();r['ads']['verified_streams'][159]['byte']=0
        with self.assertRaises(ValueError):module.validate(r)
        r=receipt();r['ads']['verified_streams'].pop()
        with self.assertRaises(ValueError):module.validate(r)
    def test_partition_shift_case_failure_or_global_policy_rejected(self):
        r=receipt();r['disks'][1]['after_partitions'][0]['PartitionNumber']=2
        with self.assertRaises(ValueError):module.validate(r)
        for key,value in [('enable_exit',1),('global_policy_changed',True),('empty',False)]:
            r=receipt();r['case'][key]=value
            with self.assertRaises(ValueError):module.validate(r)

if __name__=='__main__':unittest.main()
