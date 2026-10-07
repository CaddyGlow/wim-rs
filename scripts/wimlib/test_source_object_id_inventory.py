import copy
import importlib.util
import struct
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('inventory',Path(__file__).with_name('check-source-object-id-inventory.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
REF=(1<<48)|42
GUID=bytes(range(16))
BIRTH=bytes(range(16,64))

def audit():
    root=bytearray(136)
    struct.pack_into('<I',root,8,4096)
    struct.pack_into('<III',root,16,16,120,120)
    struct.pack_into('<HH',root,32,32,56)
    struct.pack_into('<HHH',root,40,88,16,0)
    root[48:64]=GUID;root[64:72]=REF.to_bytes(8,'little');root[72:120]=BIRTH
    struct.pack_into('<HHH',root,128,16,0,2)
    return {'records':[{'path':'$Extend/$ObjId','attributes':[{'type':'IndexRoot','name':'$O','raw_hex':root.hex()}]}]}

def inventory():return {'object_id_paths':2,'groups':[{'file_reference':REF,'object_id_full64':(GUID+BIRTH).hex()}]}

class SourceObjectIndex(unittest.TestCase):
    def test_full64_reference_and_birth_fields_match_independent_index(self):
        self.assertTrue(module.compare(inventory(),audit())['all_ordinary_full64_exact_independent_raw_index'])
    def test_changed_extended_fields_or_reference_rejected(self):
        for field,value in [('object_id_full64',(GUID+bytes(48)).hex()),('file_reference',REF+1)]:
            i=inventory();i['groups'][0][field]=value
            with self.assertRaises(ValueError):module.compare(i,audit())
    def test_ordinary_indexed_file_cannot_be_omitted(self):
        with self.assertRaisesRegex(ValueError,'omitted'):module.compare({'object_id_paths':0,'groups':[]},audit())
    def test_malformed_key_value_bounds_rejected(self):
        a=audit();b=bytearray.fromhex(a['records'][0]['attributes'][0]['raw_hex']);struct.pack_into('<H',b,32,80);a['records'][0]['attributes'][0]['raw_hex']=b.hex()
        with self.assertRaisesRegex(ValueError,'bounds'):module.read_index(a)
    def test_update_sequence_signature_mismatch_rejected(self):
        b=bytearray(512);b[:4]=b'INDX';struct.pack_into('<HH',b,4,40,2);b[40:44]=b'\xaa\xbb\xcc\xdd';b[510:512]=b'\xaa\xbb'
        self.assertEqual(module.restore_usa(b)[510:512],b'\xcc\xdd')
        b[510]=0
        with self.assertRaisesRegex(ValueError,'mismatch'):module.restore_usa(b)

if __name__=='__main__':unittest.main()
