#!/usr/bin/env python3
"""Compare wholly native new images with original wimlib verify/apply.

No WIM input fixtures or original-library capture are used by the producer.
Opaque ACL and named streams are checked by original detailed metadata listing;
Linux apply establishes data, directories, and hardlink identity only.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def run(args):
    return subprocess.run([str(a) for a in args], check=True, capture_output=True, text=True).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original", type=Path, default=Path("/tmp/wimlib-native-oracle/wimlib-imagex"))
    parser.add_argument("--producer", type=Path, default=Path("target/debug/examples/create_image"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    records = []
    content = bytes(i % 256 for i in range(153_618))
    with tempfile.TemporaryDirectory(prefix="wim-new-images-") as temp:
        root = Path(temp)
        for compression in ["none", "xpress", "lzx", "lzms"]:
            for integrity in [False, True]:
                for images in [0, 1, 2]:
                    tag = f"{compression}-{integrity}-{images}"
                    wim = root / f"{tag}.wim"
                    run([args.producer, wim, compression, str(integrity).lower(), str(images)])
                    verify = run([args.original, "verify", wim])
                    header = run([args.original, "info", wim, "--header"])
                    assert f"Boot Index                  = {images}" in header, header
                    assert "GUID                        = " + "43" * 16 in header, header
                    listings = []
                    for image in range(1, images + 1):
                        listing = run([args.original, "dir", wim, str(image), "--detailed"])
                        assert 'Named data stream "extra":' in listing, listing
                        assert hashlib.sha1(b"named stream content\n").hexdigest() in listing, listing
                        assert hashlib.sha1(content).hexdigest() in listing, listing
                        assert "Security Descriptor = 0100048000000000000000000000000000000000" in listing, listing
                        assert f"Reference Count   = {2 * images}" in listing, listing
                        assert "Link Count          = 2" in listing, listing
                        listings.append(hashlib.sha256(listing.encode()).hexdigest())
                        dest = root / f"{tag}-image{image}"
                        run([args.original, "apply", wim, str(image), dest, "--no-acls"])
                        assert (dest / "file.bin").read_bytes() == content
                        assert (dest / "alias.bin").read_bytes() == content
                        assert (dest / "file.bin").stat().st_ino == (dest / "alias.bin").stat().st_ino
                        assert (dest / "directory").is_dir()
                    records.append({"compression": compression, "integrity": integrity, "images": images,
                                    "sha256": hashlib.sha256(wim.read_bytes()).hexdigest(),
                                    "size": wim.stat().st_size, "verify": verify,
                                    "metadata_listing_sha256": listings, "apply_images": images, "passed": True})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"cases": records, "passed": len(records), "original": str(args.original)}, indent=2) + "\n")
    print(f"passed {len(records)} new-archive cases")


if __name__ == "__main__":
    main()
