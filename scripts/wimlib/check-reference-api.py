#!/usr/bin/env python3
"""Compare reference APIs, rollback/globs, and source-free split reconstruction."""
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
ORACLE=pathlib.Path('/tmp/wimlib-native-oracle')
def run(command):
    env=os.environ.copy();env['WIMLIB_DISABLE_CPU_FEATURES']='sse4.2'
    return subprocess.run([str(x) for x in command],check=True,capture_output=True,text=True,env=env).stdout

def normalized(text):
    result=[];records=[]
    for line in text.splitlines():
        if line.startswith('resource '):records.append(line)
        else:result.extend(sorted(records));records=[];result.append(line)
    result.extend(sorted(records));return result

def tree(path):
    return {str(p.relative_to(path)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(path.rglob('*')) if p.is_file()}

with tempfile.TemporaryDirectory(prefix='wim-reference-api-') as temporary:
    temp=pathlib.Path(temporary);binaries=[]
    for name,library in [('original',ORACLE/'.libs'),('native',ROOT/'target/debug')]:
        binary=temp/name
        run(['cc','-Wall','-Wextra','-Werror','-I/tmp/wimlib/include',ROOT/'scripts/wimlib/probe-reference-api.c','-L'+str(library),'-Wl,-rpath,'+str(library),'-lwim','-o',binary]);binaries.append(binary)
    source=temp/'source';source.mkdir()
    for index in range(3):(source/f'blob-{index}').write_bytes(b''.join(hashlib.sha256(f'{index}-{block}'.encode()).digest() for block in range(2200)))
    base=temp/'base.wim';tool=ORACLE/'wimlib-imagex'
    run([tool,'capture',source,base,'Reference fixture','--compress=none','--check'])
    first=temp/'refs.swm';run([tool,'split',base,first,'0.03','--check'])
    parts=sorted(temp.glob('refs*.swm'));others=[p for p in parts if p!=first]
    assert len(others)>=2
    malformed=temp/'malformed.wim';malformed.write_bytes(b'not a WIM')
    missing=temp/'missing*.swm'
    cases=[]
    for flags in [0,1,2,3,4,-1]:
        cases += [('handles-all',first,0,flags,0,others),('files-all',first,1,flags,0,others),('self',base,2,flags,0,[]),('null-entry',first,3,flags,0,[])]
    cases += [('handles-empty',first,0,0,99,[]),('files-empty',first,1,0,99,[]),('dedup',base,0,0,0,[base,base]),('repeat-parts',first,0,0,0,others+others),('one-part',first,0,0,0,others[:1]),('later-dest',others[0],0,0,0,[base]),('file-rollback-missing',first,1,0,0,[*others,temp/'missing.wim']),('file-rollback-bad',first,1,0,0,[*others,malformed]),('bad-openflags',first,1,0,8,others),('integrity-open',first,1,0,1,others)]
    for flags in [0,1,2,3]:
        cases += [('glob-all',first,1,flags,0,[temp/'refs*.swm']),('glob-missing',first,1,flags,0,[missing]),('glob-directory',first,1,flags,0,[source]),('glob-rollback',first,1,flags,0,[temp/'refs*.swm',missing])]
    cases += [(f'progress-{mode}',first,mode,0,1,others) for mode in [5,6,7,8]]
    compressible=temp/'compressible';compressible.mkdir()
    (compressible/'compressed-blob').write_bytes(bytes(range(256))*512)
    for codec,extra in [('XPRESS',[]),('LZX',[]),('LZMS',[]),('LZMS-solid',['--solid'])]:
        source_wim=temp/f'compressed-{codec}.wim'
        actual_codec=codec.split('-')[0]
        run([tool,'capture',compressible,source_wim,'Compressed references','--compress='+actual_codec,*extra])
        data=bytearray(source_wim.read_bytes())
        table_offset=int.from_bytes(data[56:64],'little');table_size=int.from_bytes(data[48:55],'little')
        metadata_records=b''.join(data[p:p+50] for p in range(table_offset,table_offset+table_size,50) if data[p+7]&2)
        assert len(metadata_records)==50 and not data[55]&4
        data[table_offset:table_offset+len(metadata_records)]=metadata_records
        data[48:55]=len(metadata_records).to_bytes(7,'little');data[64:72]=len(metadata_records).to_bytes(8,'little')
        data[124:148]=bytes(24)
        delta=temp/f'delta-{codec}.wim';delta.write_bytes(data)
        cases += [(f'{codec}-delta-handles',delta,0,0,0,[source_wim]),(f'{codec}-delta-files',delta,1,0,0,[source_wim])]
    result={'split_parts':len(parts),'cases':[],'failures':[],'source_free_outputs_applied':0,'original_cli_disabled_cpu_feature':'sse4.2'}
    for number,(label,dest,mode,flags,openflags,resources) in enumerate(cases):
        outputs=[temp/f'output-{number}-{i}.wim' for i in range(2)]
        observations=[normalized(run([binary,dest,mode,flags,openflags,output,*resources])) for binary,output in zip(binaries,outputs)]
        failures=[{'original':a,'native':b} for a,b in zip(*observations) if a!=b]
        if len(observations[0])!=len(observations[1]):failures.append({'original_lines':len(observations[0]),'native_lines':len(observations[1])})
        if not failures and 'write 0' in observations[0]:
            run([tool,'verify',outputs[1]])
            targets=[temp/f'apply-{number}-{i}' for i in range(2)]
            for output,target in zip(outputs,targets):target.mkdir();run([tool,'apply',output,1,target])
            if tree(targets[0])!=tree(targets[1]):failures.append({'applied_trees_equal':False})
            result['source_free_outputs_applied']+=1
        result['cases'].append({'case':label,'mode':mode,'flags':flags,'open_flags':openflags,'observations':len(observations[0]),'matches':not failures})
        if failures:result['failures'].append({'case':label,'flags':flags,'differences':failures[:20]})
    result['observations']=sum(case['observations'] for case in result['cases'])
    print(json.dumps(result,indent=2))
    if result['failures']:raise SystemExit(1)
