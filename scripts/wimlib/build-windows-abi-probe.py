#!/usr/bin/env python3
"""Compile an unchanged-header Windows caller; PE inspection is compile-only evidence."""
import argparse,hashlib,json,pathlib,re,struct,subprocess

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--clang',required=True,type=pathlib.Path,help='Unwrapped clang with the Windows target')
    parser.add_argument('--lld',required=True,type=pathlib.Path)
    parser.add_argument('--sdk',type=pathlib.Path,default=pathlib.Path('/home/rick/.cache/cargo-xwin/xwin'))
    parser.add_argument('--header-directory',type=pathlib.Path,default=pathlib.Path('/tmp/wimlib/include'))
    parser.add_argument('--target-directory',type=pathlib.Path,default=pathlib.Path('target/windows-abi-probe'))
    args=parser.parse_args();args.target_directory.mkdir(parents=True,exist_ok=True)
    source=pathlib.Path('scripts/wimlib/probe-windows-abi.c');obj=args.target_directory/'probe-windows-abi.obj';exe=args.target_directory/'probe-windows-abi.exe'
    compile_command=[str(args.clang),'--target=x86_64-pc-windows-msvc','-fms-extensions','-fms-compatibility','-Werror','-c',str(source),'-I'+str(args.header_directory)]
    for path in ['crt/include','sdk/include/ucrt','sdk/include/um','sdk/include/shared']: compile_command+=['-isystem',str(args.sdk/path)]
    compile_command+=['-o',str(obj)];subprocess.run(compile_command,check=True)
    link_command=[str(args.lld),'/out:'+str(exe),'/subsystem:console',str(obj)]
    link_command+=['/libpath:'+str(args.sdk/path) for path in ['crt/lib/x86_64','sdk/lib/ucrt/x86_64','sdk/lib/um/x86_64']]
    link_command+=['kernel32.lib','msvcrt.lib','vcruntime.lib','ucrt.lib','oldnames.lib'];subprocess.run(link_command,check=True)
    data=exe.read_bytes();offset=data.index(b'WIMWINABI64V1');count=struct.unpack_from('<I',data,offset+16)[0];values=struct.unpack_from('<'+str(count)+'I',data,offset+20);labels=re.findall(r' X\((\w+),',source.read_text());assert len(labels)==count
    result={'scope':'Windows MSVC-target C compilation and PE constant inspection only; not Windows execution','header_sha256':hashlib.sha256((args.header_directory/'wimlib.h').read_bytes()).hexdigest(),'probe_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'exe_sha256':hashlib.sha256(data).hexdigest(),'compiler':subprocess.check_output([str(args.clang),'--version'],text=True).splitlines()[0],'compile_command':compile_command,'link_command':link_command,'layout':dict(zip(labels,values))}
    output=pathlib.Path('docs/wimlib/evidence/native-windows-abi/compile-layout.json');output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(result,indent=2)+'\n');print(f'{count} Windows-target layout constants; {exe}; no runtime claim')
if __name__=='__main__':main()
