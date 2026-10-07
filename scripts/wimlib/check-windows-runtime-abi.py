#!/usr/bin/env python3
"""Compare recorded Windows executions, retaining missing exports as explicit gates."""
import argparse
import json
import pathlib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    root = pathlib.Path('docs/wimlib/evidence/native-windows-abi')
    parser.add_argument('--original', type=pathlib.Path, default=root / 'runtime-original.json')
    parser.add_argument('--native', type=pathlib.Path, default=root / 'runtime-native.json')
    parser.add_argument('--output', type=pathlib.Path, default=root / 'runtime-comparison.json')
    args = parser.parse_args()
    original = json.loads(args.original.read_text())
    native = json.loads(args.native.read_text())
    for execution in [original, native]:
        assert execution['C_probe']['exit'] == 0
        assert not execution['C_probe']['stdout_truncated']
        assert not execution['layout_mismatches']
        assert execution['written_WIM']['original_verify']['exit'] == 0
        assert execution['written_WIM']['original_apply']['exit'] == 0
        assert execution['written_WIM']['guid_nonzero']
    before = original['C_probe']['stdout'].splitlines()
    after = native['C_probe']['stdout'].splitlines()
    assert len(before) == len(after)
    failures = []
    export_gates = []
    matches = 0
    behavior_matches = 0
    for left, right in zip(before, after):
        if left == right:
            matches += 1
            if not left.startswith(('export ', 'export-count ')):
                behavior_matches += 1
        elif left.startswith(('export ', 'export-count ')) and right.startswith(('export ', 'export-count ')):
            export_gates.append({'original': left, 'native': right})
        else:
            failures.append({'original': left, 'native': right})
    result = {
        'scope': 'Actual Windows guest ABI/core behavior; matched CRT callers; no Windows filesystem capture/extraction claim',
        'original_record': str(args.original),
        'native_record': str(args.native),
        'rows': len(before),
        'exact_matches': matches,
        'non_export_exact_matches': behavior_matches,
        'export_gates': export_gates,
        'behavior_mismatches': failures,
    }
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    return bool(failures)


if __name__ == '__main__':
    raise SystemExit(main())
