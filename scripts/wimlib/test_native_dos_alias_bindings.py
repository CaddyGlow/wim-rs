import importlib.util
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('compare',Path(__file__).with_name('compare-native-dos-alias-bindings.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)


def row(path,alias,identity):
    return {'path_utf16_le':path,'parent_path_utf16_le':'parent','alias_utf16_le':alias,'file_id':identity}


class NativeAliasBindings(unittest.TestCase):
    def test_different_volume_identifiers_are_not_compared_to_source(self):
        result=module.compare({'aliases':[row('long','SHORT',1)]},{'aliases':[row('long','SHORT',99)]})
        self.assertTrue(result['full_source_alias_gate_passed'])

    def test_swapped_aliases_fail_even_when_all_paths_exist(self):
        source={'aliases':[row('one','~1',1),row('two','~2',2)]}
        target={'aliases':[row('one','~2',100),row('two','~1',200)]}
        result=module.compare(source,target)
        self.assertEqual(result['counts']['expected_alias_binds_other_target_file'],2)
        self.assertFalse(result['full_source_alias_gate_passed'])

    def test_same_parent_hardlinks_sharing_alias_are_not_false_collisions(self):
        result=module.compare({'aliases':[row('one','~1',1),row('two','~1',1)]},
                              {'aliases':[row('one','~1',100),row('two','~1',100)]})
        self.assertTrue(result['full_source_alias_gate_passed'])

    def test_one_alias_pointing_to_two_distinct_ids_is_rejected(self):
        with self.assertRaises(ValueError):
            module.compare({'aliases':[row('one','~1',1)]},
                           {'aliases':[row('one','~1',100),row('two','~1',200)]})

if __name__=='__main__':unittest.main()
