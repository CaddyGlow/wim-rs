import importlib.util
import struct
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('compare',Path(__file__).with_name('compare-frozen-reserved-security.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
ACE=bytes.fromhex('120b440000000000010100000000000100000000140000000200000000000000010000002800000049004d004100470045004c004f004100440000000100000000000000')
SID=bytes.fromhex('010100000000000100000000')
ACL=struct.pack('<BBHHH',2,0,8+len(ACE),1,0)+ACE


def descriptor(order,ace=ACL):
    data=bytearray(struct.pack('<BBHIIII',1,0,0x8014,0,0,0,0))
    components={'owner':SID,'group':SID,'dacl':bytes.fromhex('0200080000000000'),'sacl':ace}
    offsets={'owner':4,'group':8,'sacl':12,'dacl':16}
    for name in order:
        struct.pack_into('<I',data,offsets[name],len(data));data.extend(components[name])
    return data.hex()


class ReservedSecurityPacking(unittest.TestCase):
    def test_reordered_components_preserve_policy_without_raw_equality(self):
        source={'rows':[{'path_utf16_le':'2f00','display_path':'/','security':descriptor(['owner','group','sacl','dacl']),'reserved_aces':[ACE.hex()]}]}
        target=[{'path_utf16_le':'2f00','security':descriptor(['dacl','sacl','group','owner'])}]
        result=module.compare(source,target,source)
        self.assertEqual(result['groups']['original5727']['packing_only_difference'],1)
        self.assertEqual(result['groups']['original5727']['ace18_exact'],1)

    def test_missing_path_is_failed_not_dropped(self):
        source={'rows':[{'path_utf16_le':'2f00','display_path':'/','security':descriptor(['owner','group','sacl','dacl']),'reserved_aces':[ACE.hex()]}]}
        self.assertEqual(module.compare(source,[],source)['groups']['original5727']['missing_path'],1)

    def test_reserved_ace_loss_detected_separately_from_packing(self):
        source={'rows':[{'path_utf16_le':'2f00','display_path':'/','security':descriptor(['owner','group','sacl','dacl']),'reserved_aces':[ACE.hex()]}]}
        target=[{'path_utf16_le':'2f00','security':descriptor(['owner','group','sacl','dacl'],bytes.fromhex('0200080000000000'))}]
        result=module.compare(source,target,source)
        self.assertEqual(result['groups']['original5727']['ace18_missing'],1)
        self.assertEqual(result['groups']['original5727']['component_changed_sacl'],1)

if __name__=='__main__':unittest.main()
