#!/usr/bin/env python3
"""Compare native chunk-range reads with original private resource reader."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

p=argparse.ArgumentParser(description=__doc__)
p.add_argument("--capture",type=Path,required=True)
p.add_argument("--original",type=Path,required=True)
p.add_argument("--native",type=Path,required=True)
a=p.parse_args()
payload=bytes(range(256))*600+b"partial last chunk"
digest=hashlib.sha1(payload).hexdigest()
results=[]
with tempfile.TemporaryDirectory(prefix="wim-ranges-") as temp:
    root=Path(temp)
    source=root/"source"
    source.mkdir()
    (source/"payload.bin").write_bytes(payload)
    prefix=b"prefix"*17
    (source/"prefix.bin").write_bytes(prefix)
    for codec in ("None","XPRESS","LZX","LZMS"):
        for layout in ("ordinary","pipable","solid"):
            if codec=="None" and layout=="solid": continue
            options=[f"--compress={codec}","--no-acls","--nocheck"]
            if codec!="None": options.append("--chunk-size=32768")
            if layout=="pipable": options.append("--pipable")
            if layout=="solid": options += ["--solid",f"--solid-compress={codec}","--solid-chunk-size=32768"]
            archive=root/f"{codec}-{layout}.wim"
            subprocess.run([str(a.capture.resolve()),"capture",str(source),str(archive),*options],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
            for content in (payload,prefix):
                digest=hashlib.sha1(content).hexdigest()
                selections=[(0,0),(0,1),(1,127),(32760,20),(32768,1),(65530,100),(65536,33000),(131070,50),(len(payload)-17,17),(0,len(payload)),(len(payload),0)] if content is payload else [(0,1),(1,31),(97,5),(0,102),(102,0)]
                for offset,size in selections:
                        outputs=[]
                        for name,binary in (("original",a.original),("native",a.native)):
                            output=root/f"{name}-{codec}-{layout}-{digest}-{offset}-{size}"
                            subprocess.run([str(binary.resolve()),str(archive),digest,str(offset),str(size),str(output)],check=True)
                            outputs.append(output.read_bytes())
                        if outputs[0]!=outputs[1] or outputs[1]!=content[offset:offset+size]:
                            raise AssertionError(f"range mismatch {codec}/{layout}/{offset}/{size}")
                        results.append({"codec":codec,"layout":layout,"blob_sha1":digest,"offset":offset,"size":size,"sha256":hashlib.sha256(outputs[0]).hexdigest()})
print(json.dumps({"cases":len(results),"results":results},indent=2))
