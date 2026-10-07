#!/usr/bin/env python3
"""Exhaustively compare native NTFS uppercase against original static code."""
import argparse,hashlib,json,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs/libwim.a'));a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='wim-upcase-') as d:
 binary=Path(d)/'original';subprocess.run(['cc','scripts/wimlib/probe-ntfs-upcase.c',str(a.original),'-lpthread','-lm','-o',str(binary)],check=True)
 original=subprocess.check_output([str(binary)])
 native=subprocess.check_output(['cargo','run','--quiet','--manifest-path','Cargo.toml','--target-dir','target','--locked','-p','wim-format','--example','ntfs_upcase'])
 differences=[i for i in range(65536) if original[i*2:i*2+2]!=native[i*2:i*2+2]]
 print(json.dumps({'code_units':65536,'bytes':len(original),'equal':original==native,'sha256':hashlib.sha256(original).hexdigest(),'differences':differences},indent=2));raise SystemExit(original!=native)
