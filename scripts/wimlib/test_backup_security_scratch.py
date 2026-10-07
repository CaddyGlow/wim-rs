import copy
import hashlib
import importlib.util
import json
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('scratch',Path(__file__).with_name('prepare-backup-security-scratch.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

class BackupSecurityInput(unittest.TestCase):
    def fixture(self):
        ace=bytes.fromhex('14001800ffffff0001020000000000130000000000000000')
        sd=bytearray(20);sd[0]=1;sd[2:4]=(0x8010).to_bytes(2,'little');sd[12:16]=(20).to_bytes(4,'little')
        sd+=bytes((2,0,32,0,1,0,0,0))+ace
        return {'rows':[dict(security=sd.hex(),reserved_aces=[ace.hex()],path_utf16_le='/scratch'.encode('utf-16le').hex(),file_id=123)]}
    def run_input(self,data):
        raw=json.dumps(data).encode();return m.prepare(raw,hashlib.sha256(raw).hexdigest(),b'helper')
    def test_exact_raw_reserved_challenge_is_bound(self):
        x=self.fixture();r=self.run_input(x);self.assertEqual(r['challenges'][0]['ace_hex'],x['rows'][0]['reserved_aces'][0]);self.assertEqual(r['helper_sha256'],hashlib.sha256(b'helper').hexdigest())
    def test_hash_mismatch_rejected(self):
        with self.assertRaisesRegex(ValueError,'hash'):m.prepare(json.dumps(self.fixture()).encode(),'00'*32,b'helper')
    def test_malformed_sacl_and_ace_bounds_rejected(self):
        for offset,value in ((12,19),(22,3),(30,0)):
            x=self.fixture();sd=bytearray.fromhex(x['rows'][0]['security']);sd[offset]=value;x['rows'][0]['security']=sd.hex()
            with self.assertRaises(ValueError):self.run_input(x)
    def test_reserved_list_cannot_override_raw_descriptor(self):
        x=self.fixture();x['rows'][0]['reserved_aces']=['aa'*24]
        with self.assertRaisesRegex(ValueError,'mismatch'):self.run_input(x)

if __name__=='__main__':unittest.main()
