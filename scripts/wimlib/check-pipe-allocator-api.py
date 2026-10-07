#!/usr/bin/env python3
"""Retired custom-allocator gate; historical C probes remain upstream-only."""
import argparse
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--native',type=Path,default=Path('target/debug/libwim.so'))
p.add_argument('--output',type=Path,required=True)
p.add_argument('--maximum-failure-index',type=int,default=30)
a=p.parse_args()
raise SystemExit("Retired native allocator-hook gate: wimlib_set_memory_allocator was deliberately removed. Historical probes and evidence remain available for upstream analysis.")
