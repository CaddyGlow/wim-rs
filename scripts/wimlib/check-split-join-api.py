#!/usr/bin/env python3
"""Unchanged-header split/join comparisons, cross-joins and original extraction."""
from pathlib import Path
import hashlib
import json
import os
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[2]
ORACLE=Path('/tmp/wimlib-native-oracle')
ENV={**os.environ,'WIMLIB_DISABLE_CPU_FEATURES':'sse4.2'}
def run(args):
 result=subprocess.run([str(a) for a in args],capture_output=True,text=True,env=ENV)
 assert result.returncode==0,(args,result.returncode,result.stdout,result.stderr)
 return result.stdout
for label,library in [('original',ORACLE/'.libs'),('native',ROOT/'target/debug')]:
 run(['cc','-Wall','-Wextra','-Werror','-I','/tmp/wimlib/include',ROOT/'scripts/wimlib/probe-split-join-api.c','-L',library,f'-Wl,-rpath,{library}','-lwim','-o',f'/tmp/wim-split-join-{label}'])
def summary(path):
 b=path.read_bytes(); offset=struct.unpack_from('<Q',b,56)[0];size=int.from_bytes(b[48:55],'little')
 records=[]
 for entry in range(offset,offset+size,50):
  records.append((b[entry+30:entry+50].hex(),int.from_bytes(b[entry+16:entry+24],'little'),int.from_bytes(b[entry+26:entry+30],'little'),b[entry+7]&2))
 xo=struct.unpack_from('<Q',b,80)[0];xs=int.from_bytes(b[72:79],'little')
 return {'version':struct.unpack_from('<I',b,12)[0],'flags':struct.unpack_from('<I',b,16)[0],'chunk':struct.unpack_from('<I',b,20)[0], 'guid':b[24:40].hex(),'part':struct.unpack_from('<H',b,40)[0],'parts':struct.unpack_from('<H',b,42)[0],'images':struct.unpack_from('<I',b,44)[0],'boot':struct.unpack_from('<I',b,120)[0],'entries':sorted(records),'xml':ET.tostring(ET.fromstring(b[xo:xo+xs].decode('utf-16le')),encoding='unicode')}
cases=[]
with tempfile.TemporaryDirectory(prefix='wim-split-api-') as temporary:
 root=Path(temporary)
 for source in ['/tmp/metadata-native.wim','/tmp/wim-resource-xpress.wim','/tmp/wim-resource-pipable.wim']:
  before=hashlib.sha256(Path(source).read_bytes()).hexdigest()
  for target in [1,50,1000,1000000]:
   for integrity in [0,1,2]:
    for mutation in ['none','delete','append']:
     if source.endswith('pipable.wim') and mutation=='delete':continue
     outputs={};parts={}
     for label in ['original','native']:
      directory=root/label;directory.mkdir(exist_ok=True)
      for old in directory.glob('*.swm'):old.unlink()
      flags=2048|8|integrity
      outputs[label]=run([f'/tmp/wim-split-join-{label}','split',source,directory/'set.name.swm',target,flags,mutation])
      parts[label]=sorted(directory.glob('*.swm'),key=lambda p:summary(p)['part'])
     assert outputs['original']==outputs['native'],(source,target,integrity,mutation,outputs)
     if mutation=='append':
      assert outputs['native'].startswith('split 68\n');cases.append({'source':source,'target':target,'integrity':integrity,'mutation':mutation,'unsupported_equal':True});continue
     assert [p.name for p in parts['original']]==[p.name for p in parts['native']]
     assert [summary(p) for p in parts['original']]==[summary(p) for p in parts['native']],(source,target,integrity,mutation)
     for producer,consumer in [('original','native'),('native','original')]:
      for layout,flags in [('ordinary',integrity),('pipable',4|integrity),('solid',4096|integrity)]:
       output=root/f'joined-{producer}-{consumer}-{layout}.wim'
       result=run([f'/tmp/wim-split-join-{consumer}','join',output,0,flags,*reversed(parts[producer])]);assert result=='join 0\n',result
       run([ORACLE/'wimlib-imagex','verify',output])
       images=struct.unpack_from('<I',output.read_bytes(),44)[0]
       for image in range(1,images+1):
        directory=root/'apply';directory.mkdir(exist_ok=True);run([ORACLE/'wimlib-imagex','apply',output,image,directory]);import shutil;shutil.rmtree(directory)
     if len(parts['native'])>1:
      for invalid in [parts['native'][:-1],[parts['native'][0]]*len(parts['native'])]:
       results={label:run([f'/tmp/wim-split-join-{label}','join',root/'invalid.wim',0,0,*invalid]) for label in ['original','native']};assert set(results.values())=={'join 62\n'},results
     cases.append({'source':source,'target':target,'integrity':integrity,'mutation':mutation,'parts':len(parts['native']),'cross_joins':6,'original_verify':True,'original_apply':True})
  assert hashlib.sha256(Path(source).read_bytes()).hexdigest()==before
print(json.dumps({'count':len(cases),'cases':cases,'input_sources_preserved':True,'oracle_disabled_cpu_feature':'sse4.2'},indent=2))
