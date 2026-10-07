#!/usr/bin/env python3
"""Check ELF exports against implementation claims without declaring parity."""
import argparse
import json
from pathlib import Path
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('library', type=Path)
p.add_argument('--ledger', type=Path, default=Path('docs/wimlib/public-api-status.json'))
a = p.parse_args()
entries = json.loads(a.ledger.read_text())['symbols']
known = {entry['symbol'] for entry in entries}
optional = {'wimlib_seed_random', 'wimlib_compare_images',
            'wimlib_parse_and_write_xml_doc', 'wimlib_utf8_to_utf16le',
            'wimlib_utf16le_to_utf8'}
claimed = {entry['symbol'] for entry in entries if entry['status'] in ('partial', 'host_verified')}
lines = subprocess.check_output(['nm', '-D', '--defined-only', str(a.library)], text=True).splitlines()
exports = {line.split()[-1] for line in lines if line.split()[-1].startswith('wimlib_')}
missing = sorted(claimed - exports)
unknown = sorted(exports - known - optional)
undocumented = sorted(exports - claimed - optional)
print(json.dumps({'exported': len(exports), 'public_exported': len(exports & known), 'optional_test_support_exports': sorted(exports & optional), 'verified_claims': sum(entry['status'] == 'host_verified' for entry in entries), 'partial_claims': sum(entry['status'] == 'partial' for entry in entries), 'missing_claimed_exports': missing, 'unknown_exports': unknown, 'exports_without_implementation_claim': undocumented, 'scope': 'Linux ELF symbol presence only; optional names from original test_support.h, no ABI or behavior verification'}, indent=2))
raise SystemExit(bool(missing or unknown or undocumented))
