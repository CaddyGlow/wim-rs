import importlib.util
import unittest
from pathlib import Path
spec = importlib.util.spec_from_file_location('prepare', Path(__file__).with_name('prepare-object-id-scratch.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ObjectIdChallenge(unittest.TestCase):
    def test_exact48_copy_excludes_source_identifier(self):
        raw = bytes(range(64))
        self.assertEqual(module.extended_challenge(raw.hex()), raw[16:])

    def test_all_wrong_sizes_rejected(self):
        for size in list(range(64)) + [65, 80]:
            with self.assertRaises(ValueError):
                module.extended_challenge((b'\x11' * size).hex())

    def test_zero_source_identifier_or_birth_challenge_rejected(self):
        for raw in [bytes(16) + b'\x01'*48, b'\x01'*16 + bytes(48), b'\x01'*16 + bytes(32) + b'\x01'*16]:
            with self.assertRaises(ValueError):
                module.extended_challenge(raw.hex())

    def test_invalid_hex_rejected(self):
        with self.assertRaises(ValueError):
            module.extended_challenge('zz' * 64)

if __name__ == '__main__':
    unittest.main()
