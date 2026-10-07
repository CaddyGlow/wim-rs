#!/usr/bin/env python3
"""Compare native image selection/reordering against original verify/apply."""
import hashlib,json,pathlib,struct,subprocess,tempfile,xml.etree.ElementTree as ET
ROOT=pathlib.Path(__file__).resolve().parents[2]
ORACLE=pathlib.Path('/tmp/wimlib-native-oracle/wimlib-imagex')
EXAMPLE=ROOT/'target/debug/examples/image_select'
def run(args):
 p=subprocess.run([str(a) for a in args],capture_output=True,text=True)
 if p.returncode:raise RuntimeError(str(args)+'\n'+p.stdout+p.stderr)
 return p.stdout
def references(path):
 data=path.read_bytes();size=int.from_bytes(data[48:55],'little');offset=struct.unpack_from('<Q',data,56)[0]
 table=data[offset:offset+size];assert len(table)%50==0
 return {table[i+30:i+50].hex():struct.unpack_from('<I',table,i+26)[0] for i in range(0,len(table),50) if not table[i+7]&2}
def expected_references(selection):
 expected={}
 if selection:expected[hashlib.sha1(bytes(range(256))*300).hexdigest()]=2*len(selection)
 for index in selection:expected[hashlib.sha1(bytes([index])*10000).hexdigest()]=1
 return expected
def files(root):
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file()}
results=[]
with tempfile.TemporaryDirectory(prefix='wim-image-ops-') as tmp:
 t=pathlib.Path(tmp); source=t/'source.wim'; source_hashes=[]
 for index in range(1,4):
  tree=t/f'tree{index}';tree.mkdir();(tree/'common').write_bytes(bytes(range(256))*300)
  (tree/f'unique{index}').write_bytes(bytes([index])*10000)
  (tree/'link').hardlink_to(tree/'common');source_hashes.append(files(tree))
  run([ORACLE,'capture' if index==1 else 'append',tree,source,f'image-{index}','--compress=XPRESS'] if index==1 else [ORACLE,'append',tree,source,f'image-{index}'])
 run([ORACLE,'info',source,'2','--boot','--image-property=WINDOWS/UNKNOWN=opaque'])
 for selection in [[1],[2],[3],[3,1],[3,2,1],[1,3],[]]:
  output=t/('select-'+''.join(map(str,selection))+'.wim');run([EXAMPLE,source,output,','.join(map(str,selection)) if selection else '-'])
  run([ORACLE,'verify',output]);assert references(output)==expected_references(selection);assert struct.unpack_from('<I',output.read_bytes(),120)[0]==(selection.index(2)+1 if 2 in selection else 0);xml_path=t/'xml';run([ORACLE,'info',output,'--extract-xml='+str(xml_path)])
  xml=ET.fromstring(xml_path.read_bytes());images=xml.findall('IMAGE')
  assert [i.findtext('NAME') for i in images]==[f'image-{i}' for i in selection]
  for dest,original in enumerate(selection,1):
   extracted=t/(output.stem+f'-{dest}');run([ORACLE,'apply',output,str(dest),extracted])
   assert files(extracted)==source_hashes[original-1]
   if original==2:assert images[dest-1].findtext('WINDOWS/UNKNOWN')=='opaque'
  results.append({'selection':selection,'verified':True,'applied_images':len(selection),'sha256':hashlib.sha256(output.read_bytes()).hexdigest()})

 dest=t/'select-1.wim';out=t/'export.wim'
 run([EXAMPLE.with_name('image_export'),dest,source,out,'2,3'])
 run([ORACLE,'verify',out]);assert references(out)==expected_references([1,2,3])
 for index in range(1,4):
  extracted=t/f'export-{index}';run([ORACLE,'apply',out,str(index),extracted]);assert files(extracted)==source_hashes[index-1]
 results.append({'export_into_existing':True,'verified':True,'applied_images':3})
print(json.dumps({'cases':results},indent=2))
