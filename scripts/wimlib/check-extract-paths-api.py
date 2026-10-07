#!/usr/bin/env python3
"""Unchanged-header path/path-list extraction differential with disposable targets."""
import argparse
import hashlib
import json
import os
import pathlib
import runpy
import shutil
import subprocess
import tempfile

snapshot = runpy.run_path(str(pathlib.Path(__file__).with_name("check-extract-api.py")))["snapshot"]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--original",default="/tmp/wimlib-native-oracle")
    parser.add_argument("--native",default="target/debug")
    parser.add_argument("--output",default="docs/wimlib/evidence/native-ffi-extract-paths/differential.json")
    args=parser.parse_args()
    env={**os.environ,"WIMLIB_DISABLE_CPU_FEATURES":"sse4.2"}
    with tempfile.TemporaryDirectory(prefix="wim-extract-paths-") as temporary:
        temp=pathlib.Path(temporary)
        probes=[]
        for label,library in [("original",pathlib.Path(args.original)/".libs"),("native",pathlib.Path(args.native))]:
            library=library.resolve();probe=temp/("probe-"+label)
            subprocess.run(["cc","scripts/wimlib/probe-extract-paths-api.c","-I/tmp/wimlib/include","-L"+str(library),"-Wl,-rpath,"+str(library),"-lwim","-o",str(probe)],check=True)
            probes.append(probe)
        source=temp/"source";source.mkdir()
        for directory in ["a","b","a/sub"]:(source/directory).mkdir()
        for path,data in [("a/file",b"alpha"),("b/file",b"beta"),("a/sub/deep",b"deep"),("a/empty",b""),("case",b"lower"),("CASE",b"upper"),("a/literal[*]",b"brackets"),("unicode-😀",b"unicode")]:
            (source/path).write_bytes(data)
        os.link(source/"a/file",source/"b/alias")
        os.symlink("file",source/"a/link")
        for path in [source,*source.rglob("*")]:os.utime(path,ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100),follow_symlinks=False)
        original=pathlib.Path(args.original)/"wimlib-imagex"
        wim=temp/"source.wim"
        subprocess.run([str(original),"capture",str(source),str(wim),"Fixture","--compress=XPRESS","--unix-data"],env=env,stdout=subprocess.DEVNULL,check=True)
        original_digest=hashlib.sha256(wim.read_bytes()).hexdigest()
        cases=[]
        selections=[[],["/"],["a/file"],["a"],["a/sub/deep","b/alias"],["a/file","a/file"],["a/file","a"],["b","a"],["a/file","b/file"],["a/file","b/alias"],["a/link"],["a/empty"],["missing"],["a/file/child"],["a/link/child"],["./a/file"],["a/../b/file"],["\\a\\file\\"],["a///file///"],[""],["A/file"],["case"],["unicode-😀"],["a/*"],["*"],["a/?ile"],["a/literal[*]"],["a/no-match*"],["no*","a/file"],["unicode-?"],["unicode-????"]]
        for paths in selections:
            for flags in [0,0x200000,0x40000,0xc0000]:cases.append((0,1,flags,0,paths,b""))
        for paths in [["a/file"],["a/empty"],["a/file","a/file"],["a/file","b/file"],["a/file","missing"],["a"],["a/link"],["a/*"]]:
            for flags in [0x400,0x200400,0x40400]:cases.append((0,1,flags,0,paths,b""))
        for image in [0,2,-1]:
            for flags in [0,1,0xc0,0x400000,4]:cases.append((0,image,flags,0,["a/file"],b""))
        for status in [1,2,103,104,106,108,204]:cases.append((0,1,0,status,["a/file"],b""))
        lists=[b"# comment\n ; comment\n'a/file'\n\"a/sub/deep\"\r\n",b"a/file",b"\n",b"''\n",b"[section]\na/file\n",b"a/file\0ignored\nb/file\n",b"a/file # literal comment\n",b'"a/file\n',b"a/*\n",b"\xef\xbb\xbfa/file\n",b"\xff\xfe"+"a/file\n".encode("utf-16le"),b"\xff\xfeX"]
        for index,contents in enumerate(lists):
            path=temp/f"list-{index}.txt";path.write_bytes(contents)
            for flags in [0,0x200000,0x40000]:cases.append((1,1,flags,0,[str(path)],b""))
            cases.append((1,1,0,0,[],contents))
            cases.append((1,1,0,0,["-"],contents))
        cases.append((1,1,4,0,[str(temp/"missing-list")],b""))
        for paths in [[],["/"],["*"],["missing"]]:
            for flags in [0,0x200000,0x40000,0xc0000]:cases.append((2,1,flags,0,paths,b""))
        cases.append((3,1,0,0,[],b""))
        cases.append((3,-1,0,0,[],b""))
        dash_cwd=temp/"dash-cwd";dash_cwd.mkdir();(dash_cwd/"-").write_bytes(b"'a/file'\n")
        cases.append((1,1,0,0,["-"],b"missing\n",{"cwd":str(dash_cwd)}))
        for paths in [["A/file"],["cAsE"],["a/FILE"],["CASE"],["A/*"],["unicode-😀"],["\udcff"]]:
            for flags in [0,0x40000,0xc0000]:cases.append((256,1,flags,0,paths,b""))
        for mode in [512,1024,2048,4096,8192]:
            for flags in [0,1,4]:cases.append((mode,1,flags,0,["a/file"],b""))
        for shape in ["file_target","parent_symlink","symlink_target","missing_parent","directory_leaf"]:
            for paths in [["a/file"],["a"],["a/empty"],["a/link"]]:
                for flags in [0,0x200000]:cases.append((0,1,flags,0,paths,b"",{"shape":shape}))
        for variant,extra in [("solid",["--solid"]),("pipable",["--pipable"])]:
            special=temp/(variant+".wim")
            subprocess.run([str(original),"capture",str(source),str(special),"Fixture","--compress=LZMS" if variant=="solid" else "--compress=XPRESS",*extra],env=env,stdout=subprocess.DEVNULL,check=True)
            for paths in selections[:12]:
                for flags in [0,0x200000,0x40000]:cases.append((0,1,flags,0,paths,b"",{"wim":str(special)}))
        for fixture in pathlib.Path("/tmp/wimlib/tests/wims").glob("*.wim"):
            for flags in [0,32,128]:cases.append((0,1,flags,0,["/"],b"",{"wim":str(fixture)}))
            if fixture.stem.startswith("corrupted_file"):
                for flags in [0,2,0x400,0x402]:cases.append((0,1,flags,0,["file"],b"",{"wim":str(fixture)}))
        solid_source=temp/"solid-source";shutil.copytree(source,solid_source,symlinks=True)
        (solid_source/"a/large").write_bytes(b"alpha payload"*6000)
        (solid_source/"b/large").write_bytes(b"beta payload"*5000)
        for path in [solid_source,*solid_source.rglob("*")]:os.utime(path,ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100),follow_symlinks=False)
        solid_wim=temp/"solid-small-chunks.wim"
        subprocess.run([str(original),"capture",str(solid_source),str(solid_wim),"Solid chunks","--compress=LZMS","--solid","--solid-chunk-size=32768"],env=env,stdout=subprocess.DEVNULL,check=True)
        for paths in [["a"],["b"],["a","b"],["a/large"],["b/large"]]:
            for flags in [0,0x200000,0x40000]:cases.append((0,1,flags,0,paths,b"",{"wim":str(solid_wim)}))
        mismatches=[];observations=0
        outside=temp/"outside";outside.mkdir();(outside/"sentinel").write_bytes(b"preserved")
        for index,case in enumerate(cases):
            mode,image,flags,status,paths,stdin=case[:6]
            extra=case[6] if len(case)==7 else {}
            outputs=[];trees=[]
            for label,probe in zip(["original","native"],probes):
                target=temp/f"target-{index}-{label}"
                shape=extra.get("shape")
                if shape=="symlink_target":
                    shutil.rmtree(outside);outside.mkdir();(outside/"sentinel").write_bytes(b"preserved")
                    os.utime(outside/"sentinel",ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100))
                if shape=="file_target":target.write_bytes(b"existing target")
                elif shape=="parent_symlink":target.mkdir();os.symlink(outside,target/"a")
                elif shape=="symlink_target":os.symlink(outside,target)
                elif shape=="missing_parent":target=target/"missing-target"
                elif shape=="directory_leaf":target.mkdir();(target/("file" if flags & 0x200000 else "a")).mkdir();(target/"a/file").mkdir() if not flags & 0x200000 else None
                if shape and shape!="missing_parent":
                    for path in [target,*target.rglob("*")] if target.is_dir() and not target.is_symlink() else [target]:
                        os.utime(path,ns=(1_600_000_000_123_456_700,1_600_000_001_765_432_100),follow_symlinks=False)
                process=subprocess.run([str(probe),extra.get("wim",str(wim)),str(mode),str(image),str(target),str(flags),str(status),*paths],input=stdin,env=env,cwd=extra.get("cwd"),capture_output=True,check=True)
                outputs.append(process.stdout)
                # Flattened non-root selections use a newly created container,
                # whose filesystem timestamps are not restored by either library.
                container=bool(flags & 0x200000) and "/" not in paths and "" not in paths
                tree=snapshot(target,container,b"extract 0\n" not in process.stdout)
                if shape=="symlink_target":
                    tree[0]["atime"]="caller target symlink traversal time"
                    for entry in snapshot(outside,container,b"extract 0\n" not in process.stdout):
                        entry["path"]="outside:"+entry["path"];tree.append(entry)
                trees.append(tree)
            observations+=len(outputs[0].splitlines())
            if outputs[0]!=outputs[1] or trees[0]!=trees[1]:
                mismatches.append({"case":index,"mode":mode,"image":image,"flags":flags,"status":status,"paths":paths,"stdin_hex":stdin.hex(),"original_output_hex":outputs[0].hex(),"native_output_hex":outputs[1].hex(),"original_tree":trees[0],"native_tree":trees[1]})
        assert hashlib.sha256(wim.read_bytes()).hexdigest()==original_digest
        output=pathlib.Path(args.output);output.parent.mkdir(parents=True,exist_ok=True)
        output.write_text(json.dumps({"cases":len(cases),"observations":observations,"mismatches":mismatches,"source_preserved":True,"oracle_cpu_workaround":"WIMLIB_DISABLE_CPU_FEATURES=sse4.2"},indent=2)+"\n")
        print(f"{len(cases)} cases; {observations} observations; {len(mismatches)} mismatches; {output}")
        if mismatches:raise SystemExit(1)


if __name__=="__main__":main()
