#!/usr/bin/env python3
"""Reusable native compressor blocks read by unchanged original codecs."""
import argparse,subprocess,tempfile,shutil,json,os
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--native',type=Path,required=True);p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'));a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='wim-compressor-workspace-') as d:
 t=Path(d);n=t/'native.so';shutil.copy2(a.native/'libwim.so',n);binary=t/'probe'
 subprocess.run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include','scripts/wimlib/probe-compressor-workspace-api.c','-ldl','-o',str(binary)],check=True)
 output=subprocess.check_output([str(binary),str(n),(a.original/'libwim.so').resolve()],env=dict(os.environ,WIMLIB_DISABLE_CPU_FEATURES='sse4.2'))
 result=json.loads(output);assert result['native_compressor_original_decoder_cases']==90;assert result['factory_original_comparisons']==120;print(json.dumps(result,indent=2))
