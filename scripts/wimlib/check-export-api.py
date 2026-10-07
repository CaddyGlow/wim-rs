#!/usr/bin/env python3
"""Original C ABI differential export and source-free/write/apply checks."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser()
parser.add_argument('--oracle',type=pathlib.Path,default=pathlib.Path('/tmp/wimlib-native-oracle'))
args=parser.parse_args()
def run(command):
    return subprocess.run([str(v) for v in command],check=True,capture_output=True,text=True).stdout

def normalized(output):
    result=[];resources=[]
    for line in output.splitlines():
        if line.startswith('resource '):resources.append(line)
        else:
            result.extend(sorted(resources));resources=[];result.append(line)
    result.extend(sorted(resources));return result

def tree(path):
    return {str(p.relative_to(path)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(path.rglob('*')) if p.is_file()}

with tempfile.TemporaryDirectory(prefix='wim-export-api-') as temporary:
    temp=pathlib.Path(temporary)
    binaries=[]
    for label,lib in [('original',args.oracle/'.libs'),('native',ROOT/'target/debug')]:
        binary=temp/label
        run(['cc','-I/tmp/wimlib/include',ROOT/'scripts/wimlib/probe-export-api.c','-L'+str(lib),'-Wl,-rpath,'+str(lib),'-lwim','-o',binary])
        binaries.append(binary)
    source=temp/'source';source.mkdir()
    (source/'shared').write_bytes(b'export ownership\n'*512)
    os.link(source/'shared',source/'alias')
    tool=args.oracle/'wimlib-imagex'
    fixtures=[]
    for codec in ['none','XPRESS','LZX','LZMS']:
        path=temp/(codec+'.wim')
        run([tool,'capture',source,path,'First','Source description','--compress='+codec])
        run([tool,'append',source,path,'Second','Second description','--boot'])
        fixtures.append((codec,path))
    result={'cases':[],'failures':[],'source_free_outputs_applied':0}
    combinations=[(flags,image,mode) for flags in range(32) for image in [1,-1] for mode in [0,1,2,3,4]]
    # Every flag bitmap and parameter mode on the uncompressed source; remaining
    # codec fixtures cover normal/gift/all-image handling plus pending empty trees.
    for label,path in fixtures:
        cases=combinations if label=='none' else [(flags,image,0) for flags in [0,1,2,4,8,16,31] for image in [1,-1,3]]
        for flags,image,mode in cases:
            outputs=[];lines=[]
            for binary in binaries:
                output=temp/f'{binary.name}-{label}-{flags}-{image}-{mode}.wim'
                lines.append(normalized(run([binary,path,flags,image,mode,output])))
                outputs.append(output)
            failures=[{'original':a,'native':b} for a,b in zip(*lines) if a!=b]
            if len(lines[0])!=len(lines[1]):failures.append({'original_lines':len(lines[0]),'native_lines':len(lines[1])})
            applied=False
            if outputs[0].exists() and outputs[1].exists() and not failures:
                run([tool,'verify',outputs[1]])
                image_count=int(next(line for line in lines[0] if line.startswith('state dest ')).split()[2])
                for index in range(1,image_count+1):
                    targets=[]
                    for output in outputs:
                        target=temp/f'apply-{output.stem}-{index}';target.mkdir()
                        run([tool,'apply',output,index,target]);targets.append(target)
                    if tree(targets[0])!=tree(targets[1]):failures.append({'apply_image':index,'trees_equal':False})
                result['source_free_outputs_applied']+=1;applied=True
            result['cases'].append({'codec':label,'flags':flags,'image':image,'mode':mode,'observations':len(lines[0]),'matches':not failures,'applied':applied})
            if failures:result['failures'].append({'codec':label,'flags':flags,'image':image,'mode':mode,'differences':failures[:20]})
    for name in ['corrupted_file_1.wim','corrupted_file_2.wim','cyclic.wim']:
        path=pathlib.Path('/tmp/wimlib/tests/wims')/name
        outputs=[temp/f'bad-{i}-{name}' for i in range(2)]
        lines=[normalized(run([binary,path,0,1,0,output])) for binary,output in zip(binaries,outputs)]
        failures=[{'original':a,'native':b} for a,b in zip(*lines) if a!=b]
        if len(lines[0])!=len(lines[1]):failures.append({'original_lines':len(lines[0]),'native_lines':len(lines[1])})
        result['cases'].append({'fixture':name,'observations':len(lines[0]),'matches':not failures})
        if failures:result['failures'].append({'fixture':name,'differences':failures[:20]})
    result['observations']=sum(case['observations'] for case in result['cases'])
    print(json.dumps(result,indent=2))
    if result['failures']:raise SystemExit(1)
