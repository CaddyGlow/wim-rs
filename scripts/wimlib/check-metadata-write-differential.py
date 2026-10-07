#!/usr/bin/env python3
"""Capture with upstream, regenerate metadata natively, then iterate and apply upstream."""
import argparse, hashlib, json, pathlib, subprocess, tempfile
p=argparse.ArgumentParser();p.add_argument('--oracle',default='/tmp/wimlib-native-oracle/wimlib-imagex');p.add_argument('--writer',default='target/debug/examples/rewrite_metadata');p.add_argument('--output',required=True);a=p.parse_args()
def run(args):return subprocess.run(list(map(str,args)),check=True,capture_output=True).stdout
with tempfile.TemporaryDirectory(prefix='metadata-write-') as d:
 d=pathlib.Path(d);src=d/'src';src.mkdir();(src/'sub').mkdir();(src/'sub'/'file').write_bytes(bytes(range(256))*30);(src/'empty').touch();(src/'alias').hardlink_to(src/'sub'/'file');(src/'link').symlink_to('sub/file')
 results=[]
 for unix in [False,True]:
  w=d/'original.wim';w.unlink(missing_ok=True);run([a.oracle,'capture',src,w,'image','--compress=none']+(['--unix-data'] if unix else []));b=bytearray(w.read_bytes());table=int.from_bytes(b[56:64],'little');size=int.from_bytes(b[48:55],'little')
  row=next(r for r in range(table,table+size,50) if b[r+7]&2);start=int.from_bytes(b[row+8:row+16],'little');length=int.from_bytes(b[row:row+7],'little');raw=d/'old.bin';out=d/'new.bin';raw.write_bytes(b[start:start+length]);run([a.writer,raw,out]);new=out.read_bytes();offset=len(b);b.extend(new);b[row:row+7]=len(new).to_bytes(7,'little');b[row+8:row+16]=offset.to_bytes(8,'little');b[row+16:row+24]=len(new).to_bytes(8,'little');b[row+30:row+50]=hashlib.sha1(new).digest();nw=d/'new.wim';nw.write_bytes(b)
  before=run([a.oracle,'dir',w]);after=run([a.oracle,'dir',nw]);assert before==after
  run([a.oracle,'verify',nw]);dest=d/f'apply-{unix}';run([a.oracle,'apply',nw,'1',dest]+(['--unix-data'] if unix else []));assert (dest/'sub'/'file').read_bytes()==(src/'sub'/'file').read_bytes();assert (dest/'alias').stat().st_ino==(dest/'sub'/'file').stat().st_ino;assert (dest/'link').readlink()==pathlib.Path('sub/file')
  results.append(dict(unix_data=unix,original_size=length,native_size=len(new),native_sha1=hashlib.sha1(new).hexdigest(),directory_equal=True,verify=True,apply_payload=True,hardlinks=True,symlink=True))
 pathlib.Path(a.output).write_text(json.dumps(results,indent=2)+'\n')
 print('matched original iteration, verification and application:',len(results),'metadata variants')
