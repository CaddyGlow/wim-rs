#!/usr/bin/env python3
"""Retired custom-allocator gate; historical C probes remain upstream-only."""
import argparse
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--native',type=Path,required=True);p.add_argument('--original',type=Path,default=Path('/tmp/wimlib-native-oracle/.libs'));a=p.parse_args()
raise SystemExit("Retired native allocator-hook gate: wimlib_set_memory_allocator was deliberately removed. Historical probes and evidence remain available for upstream analysis.")
