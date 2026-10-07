import importlib.util
import struct
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('prepare', Path(__file__).with_name('prepare-resource-attribute-experiment.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

# Copied immutable IMAGELOAD UINT64=1 claim, Everyone SID.
ACE = bytes.fromhex('120b440000000000010100000000000100000000140000000200000000000000010000002800000049004d004100470045004c004f004100440000000100000000000000')

class ResourceAttributeBounds(unittest.TestCase):
    def test_frozen_source_claim_is_accepted(self):
        self.assertEqual(module.validate_ace18(ACE), ACE)

    def test_other_ace_types_are_rejected(self):
        for kind in (0, 17, 20):
            with self.assertRaises(ValueError):
                module.validate_ace18(bytes([kind]) + ACE[1:])

    def test_truncation_and_wrong_sizes_are_rejected(self):
        for size in range(len(ACE)):
            with self.assertRaises(ValueError):
                module.validate_ace18(ACE[:size])

    def test_claim_offsets_cannot_escape_buffer(self):
        for position in (20, 36):
            raw = bytearray(ACE)
            struct.pack_into('<I', raw, position, 0xffffffff)
            with self.assertRaises(ValueError):
                module.validate_ace18(bytes(raw))

    def test_misaligned_claim_offsets_are_rejected(self):
        for position, offset in ((20, 21), (36, 41)):
            raw = bytearray(ACE)
            struct.pack_into('<I', raw, position, offset)
            with self.assertRaises(ValueError):
                module.validate_ace18(bytes(raw))

    def test_nonzero_masks_and_invalid_sid_are_rejected(self):
        for position, value in ((4, 1), (8, 2), (9, 16), (1, 128)):
            raw = bytearray(ACE); raw[position] = value
            with self.assertRaises(ValueError):
                module.validate_ace18(bytes(raw))

if __name__ == '__main__':
    unittest.main()
