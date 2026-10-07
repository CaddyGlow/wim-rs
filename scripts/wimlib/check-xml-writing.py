#!/usr/bin/env python3
"""Compare original/native writer XML size policy in all native layouts."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET

p=argparse.ArgumentParser(description=__doc__)
p.add_argument("--oracle",type=Path,required=True)
p.add_argument("--native-dir",type=Path,required=True)
a=p.parse_args()

def run(args):
    subprocess.run([str(x) for x in args],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)

def statistics(path):
    data=path.read_bytes()
    pipable=data[:8]==b"WLPWM\0\0\0"
    header=data[-208:] if pipable else data[:208]
    table_end=int.from_bytes(header[56:64],"little")+int.from_bytes(header[48:55],"little")
    xml_offset=int.from_bytes(header[80:88],"little")
    xml_length=int.from_bytes(header[88:96],"little")
    xml=ET.fromstring(data[xml_offset:xml_offset+xml_length].decode("utf-16"))
    declared=int(xml.findtext("TOTALBYTES"))
    if declared!=table_end: raise AssertionError(f"stored-byte mismatch {path}")
    if pipable:
        length=int.from_bytes(data[216:224],"little")
        preliminary=ET.fromstring(data[248:248+length].decode("utf-16"))
        if preliminary.find("TOTALBYTES") is not None: raise AssertionError("preliminary TOTALBYTES present")
    return {"table_end":table_end,"xml_totalbytes":declared,"preliminary_omitted":pipable}

results=[]
with tempfile.TemporaryDirectory(prefix="wim-xml-write-") as directory:
    root=Path(directory)
    source=root/"source"
    source.mkdir()
    (source/"payload").write_bytes(bytes(range(256))*300+b"end")
    original=root/"source.wim"
    run([a.oracle.resolve(),"capture",source,original,"--no-acls","--nocheck"])
    for codec in ("None","XPRESS","LZX","LZMS"):
        for layout in ("ordinary","pipable","solid"):
            if codec=="None" and layout=="solid":continue
            reference=root/f"original-{codec}-{layout}.wim"
            candidate=root/f"native-{codec}-{layout}.wim"
            flags=[f"--compress={codec}","--nocheck"]
            if layout=="pipable":flags.append("--pipable")
            if layout=="solid":flags += ["--solid",f"--solid-compress={codec}"]
            run([a.oracle.resolve(),"export",original,"all",reference,*flags])
            if layout=="solid":
                command=[a.native_dir.resolve()/"solid_repack",original,candidate,codec,"no-integrity"]
            else:
                command=[a.native_dir.resolve()/("repack_pipable" if layout=="pipable" else "repack"),original,candidate,"no-integrity",codec]
            run(command)
            results.append({"codec":codec,"layout":layout,"original":statistics(reference),"native":statistics(candidate)})
print(json.dumps({"cases":len(results),"results":results},indent=2))
