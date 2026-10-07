#!/usr/bin/env python3
"""Compare native selector resolution with the original public library API."""
import argparse
import ctypes as c
import json
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--oracle', type=Path, required=True)
p.add_argument('--native', type=Path, required=True)
a = p.parse_args()
lib = c.CDLL(str(a.oracle.resolve()))
lib.wimlib_create_new_wim.argtypes = [c.c_int, c.POINTER(c.c_void_p)]
lib.wimlib_add_empty_image.argtypes = [c.c_void_p, c.c_char_p, c.POINTER(c.c_int)]
lib.wimlib_resolve_image.argtypes = [c.c_void_p, c.c_char_p]
lib.wimlib_free.argtypes = [c.c_void_p]
selectors = ['', 'all', 'ALL', 'aLl', '*', ' all', 'all ', '1', '01', '+1', ' +02', '\t\n3', '1 ', '0', '+0', '-0', '-1', '3', '4', 'Alpha', 'alpha', '😀', '1x', '999999999999999999999999999999', '18446744073709551616']
names = ['0', '3', 'Alpha', '😀', '-1', '1 ', '999999999999999999999999999999']
handle = c.c_void_p()
assert lib.wimlib_create_new_wim(0, c.byref(handle)) == 0
try:
    for name in names:
        assert lib.wimlib_add_empty_image(handle, name.encode(), None) == 0
    expected = [lib.wimlib_resolve_image(handle, value.encode()) for value in selectors]
    assert lib.wimlib_resolve_image(handle, None) == 0
    with tempfile.TemporaryDirectory(prefix='wim-selector-') as temp:
        xml = Path(temp) / 'images.xml'
        xml.write_text('<WIM>' + ''.join(f'<IMAGE INDEX="{i}"><NAME>{name}</NAME></IMAGE>' for i, name in enumerate(names, 1)) + '</WIM>')
        actual = [int(value) for value in subprocess.check_output([str(a.native.resolve()), str(xml), *selectors], text=True).splitlines()]
    assert actual == expected, list(zip(selectors, expected, actual))
    print(json.dumps({'cases': len(selectors) + 1, 'selectors': selectors, 'expected': expected, 'native': actual, 'passed': True}, indent=2))
finally:
    lib.wimlib_free(handle)
