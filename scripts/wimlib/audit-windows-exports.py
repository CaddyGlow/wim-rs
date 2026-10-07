#!/usr/bin/env python3
"""Audit actual PE exports against the public ledger without claiming Windows parity."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('library', type=Path)
parser.add_argument('--ledger', type=Path, default=Path('docs/wimlib/public-api-status.json'))
parser.add_argument('--output', type=Path)
args = parser.parse_args()
raw = args.library.read_bytes()
if raw[:2] != b'MZ':
    parser.error('library is not a PE image')
listing = subprocess.check_output(['objdump', '-p', str(args.library)], text=True)
exports = set(re.findall(r'^\s*\[\s*\d+\].*\s(wimlib_\w+)\s*$', listing, re.MULTILINE))
if not exports:
    parser.error('no wimlib exports parsed; inspect the PE export table format')
entries = json.loads(args.ledger.read_text())['symbols']
removed = {entry['symbol'] for entry in entries if entry['status'] == 'removed'}
known = {entry['symbol'] for entry in entries} - removed
optional = {'wimlib_seed_random', 'wimlib_compare_images',
            'wimlib_parse_and_write_xml_doc', 'wimlib_utf8_to_utf16le',
            'wimlib_utf16le_to_utf8'}
result = dict(scope='Windows PE linkage/export presence only; no runtime or ABI behavior claim',
              dll_sha256=hashlib.sha256(raw).hexdigest(), exported=len(exports),
              missing=sorted(known - exports), unknown=sorted(exports - known - optional),
              deliberately_removed=sorted(removed),
              public_exported=len(exports & known), optional_test_support_exports=sorted(exports & optional),
              symbols=sorted(exports))
if args.output:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({key: value for key, value in result.items() if key != 'symbols'}, indent=2))
raise SystemExit(bool(result['missing'] or result['unknown']))
