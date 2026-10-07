#!/usr/bin/env python3
"""Compare actual MSVC-target Rust PE constants with independently compiled C layouts."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct

from pe_inspect import inspect_pe

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--rust-exe', type=Path, default=Path('target/x86_64-pc-windows-msvc/debug/examples/abi_layout.exe'))
parser.add_argument('--c-layout', type=Path, default=Path('docs/wimlib/evidence/native-windows-abi/compile-layout.json'))
parser.add_argument('--output', type=Path, default=Path('docs/wimlib/evidence/native-windows-abi/compile-rust-comparison.json'))
args = parser.parse_args()
source = Path('crates/wim/examples/abi_layout.rs')
labels = re.findall(r'"([a-z_]+)"', source.read_text().split('const LABELS:', 1)[1].split('];', 1)[0])
raw = args.rust_exe.read_bytes()
metadata = inspect_pe(args.rust_exe)
values = None
for section in metadata['sections']:
    if section['name'] == '.wlabi':
        size, offset = section['raw_size'], section['raw_offset']
        if size < 8 * len(labels) or offset + 8 * len(labels) > len(raw):
            parser.error('truncated Rust layout section')
        values = struct.unpack_from('<' + 'Q' * len(labels), raw, offset)
        break
if values is None:
    parser.error('Rust layout section missing')
c = json.loads(args.c_layout.read_text())['layout']
native = dict(zip(labels, values))
differences = {name: dict(c=c.get(name), rust=value) for name, value in native.items() if c.get(name) != value}
result = dict(scope='Windows MSVC-target compile-time layout only; no runtime behavior claim',
              rust_exe_sha256=hashlib.sha256(raw).hexdigest(),
              rust_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
              c_layout_sha256=hashlib.sha256(args.c_layout.read_bytes()).hexdigest(),
              compared=len(native), exact=len(native) - len(differences), differences=differences,
              unmeasured_c_fields=sorted(set(c) - set(native)), rust_layout=native)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({key: value for key, value in result.items() if key != 'rust_layout'}, indent=2))
raise SystemExit(bool(differences))
