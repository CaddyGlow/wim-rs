#!/usr/bin/env python3
"""Package a reviewed Linux facade artifact locally without installing it system-wide."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--library', type=Path, default=Path('target/debug/libwim.so'))
parser.add_argument('--output', type=Path)
args = parser.parse_args()
raw = args.library.read_bytes()
digest = hashlib.sha256(raw).hexdigest()
dynamic = subprocess.check_output(['readelf', '-d', str(args.library)], text=True)
if 'Library soname: [libwim.so.15]' not in dynamic:
    parser.error('artifact does not have the required libwim.so.15 SONAME')
subprocess.run(['python3', 'scripts/wimlib/audit-native-exports.py', str(args.library)], check=True)
output = args.output or Path('dist/wimlib-rs') / ('linux-' + digest[:16])
if output.exists():
    parser.error('output already exists; choose a fresh local package destination')
output.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='wim-package-', dir=output.parent) as temporary:
    stage = Path(temporary) / 'package'
    (stage / 'lib').mkdir(parents=True)
    (stage / 'include').mkdir()
    shutil.copyfile(args.library, stage / 'lib/libwim.so.15')
    (stage / 'lib/libwim.so').symlink_to('libwim.so.15')
    shutil.copyfile('/tmp/wimlib/include/wimlib.h', stage / 'include/wimlib.h')
    shutil.copyfile('/tmp/wimlib/COPYING.LGPL', stage / 'COPYING.LGPL')
    shutil.copyfile('crates/wim/README.md', stage / 'IMPLEMENTATION-STATUS.md')
    manifest = dict(scope='Local experimental Linux facade package; behavioral/platform gates remain incomplete',
                    library_sha256=digest, soname='libwim.so.15',
                    header_sha256=hashlib.sha256((stage / 'include/wimlib.h').read_bytes()).hexdigest(),
                    source_baseline='cd5e231c348c255ae5088873b5a66ee0eb96fa07',
                    system_install=False)
    (stage / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    stage.rename(output)
print(json.dumps(dict(output=str(output.resolve()), **manifest), indent=2))
