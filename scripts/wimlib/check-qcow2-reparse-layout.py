#!/usr/bin/env python3
"""Independent raw WIM layout oracle using upstream libwim chunk decompression.

Reads on-disk main_hash and extra stream records directly; tolerant tree readers
can accept a noncanonical form that Windows Setup rejects. Never writes the WIM.
Reference: upstream wimlib src/dentry.c write_dentry_streams rules.
"""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import struct

ZERO = bytes(20)
LIMIT = 256 * 1024 * 1024


def u16(data, offset):
    return struct.unpack_from('<H', data, offset)[0]


def u32(data, offset):
    return struct.unpack_from('<I', data, offset)[0]


def u64(data, offset):
    return struct.unpack_from('<Q', data, offset)[0]


def descriptor(data):
    return (int.from_bytes(data[:7], 'little'), data[7], u64(data, 8), u64(data, 16))


class Archive:
    def __init__(self, path, library):
        self.file = Path(path).open('rb')
        self.length = Path(path).stat().st_size
        self.library = ctypes.CDLL(str(library))
        self.library.wimlib_create_decompressor.argtypes = [ctypes.c_int, ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p)]
        self.library.wimlib_decompress.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        self.library.wimlib_free_decompressor.argtypes = [ctypes.c_void_p]
        header = self.read(0, 208)
        if header[:8] != b'MSWIM\0\0\0' or u32(header, 8) != 208:
            raise ValueError('requires ordinary WIM header')
        flags = u32(header, 16)
        self.codec = 2 if flags & 0x40000 else 1 if flags & 0x20000 else 3 if flags & 0x80000 else 0
        self.chunk = u32(header, 20)
        if not self.codec or self.chunk < 4096 or self.chunk > 1024 * 1024 or self.chunk & (self.chunk - 1):
            raise ValueError('unsupported codec/chunk configuration')
        self.decoder = ctypes.c_void_p()
        if self.library.wimlib_create_decompressor(self.codec, self.chunk, ctypes.byref(self.decoder)):
            raise ValueError('upstream decompressor construction failed')
        table = self.resource(descriptor(header[48:72]))
        if len(table) % 50:
            raise ValueError('truncated blob table')
        metadata = []
        self.blobs = {}
        for offset in range(0, len(table), 50):
            record = table[offset:offset + 50]
            resource = descriptor(record)
            if resource[1] & 16:
                raise ValueError('solid resources not supported by this bounded oracle')
            digest = record[30:50]
            if resource[1] & 2:
                metadata.append((resource, digest))
            else:
                self.blobs[digest] = resource
        if len(metadata) != 1:
            raise ValueError('requires one captured image')
        self.metadata = self.resource(metadata[0][0])
        if hashlib.sha1(self.metadata).digest() != metadata[0][1]:
            raise ValueError('metadata SHA-1 mismatch')

    def close(self):
        self.library.wimlib_free_decompressor(self.decoder)
        self.file.close()

    def read(self, offset, size):
        if size > LIMIT or offset < 0 or offset + size > self.length:
            raise ValueError('bounded read exceeds WIM')
        self.file.seek(offset)
        data = self.file.read(size)
        if len(data) != size:
            raise ValueError('short read')
        return data

    def resource(self, item):
        size, flags, offset, logical = item
        if logical > LIMIT:
            raise ValueError('resource exceeds oracle output bound')
        encoded = self.read(offset, size)
        if not flags & 4:
            if logical != size:
                raise ValueError('uncompressed length mismatch')
            return encoded
        count = (logical + self.chunk - 1) // self.chunk
        width = 4 if logical <= 0xffffffff else 8
        table_size = (count - 1) * width
        offsets = [0] + [int.from_bytes(encoded[i:i + width], 'little') for i in range(0, table_size, width)] + [size - table_size]
        result = bytearray()
        for index in range(count):
            start, end = offsets[index:index + 2]
            if start < 0 or end < start or table_size + end > size:
                raise ValueError('bad resource chunk span')
            packed = encoded[table_size + start:table_size + end]
            expected = min(self.chunk, logical - len(result))
            if len(packed) == expected:
                result.extend(packed)
            else:
                source = ctypes.create_string_buffer(packed)
                output = ctypes.create_string_buffer(expected)
                if self.library.wimlib_decompress(source, len(packed), output, expected, self.decoder):
                    raise ValueError('upstream decompression failure')
                result.extend(output.raw)
        if len(result) != logical:
            raise ValueError('resource output length mismatch')
        return bytes(result)

    def blob(self, digest):
        if digest == ZERO:
            return b''
        data = self.resource(self.blobs[digest])
        if hashlib.sha1(data).digest() != digest:
            raise ValueError('resource SHA-1 mismatch')
        return data

    def nodes(self):
        for node in self.nodes_with_raw_records():
            yield node[:5]

    def nodes_with_raw_records(self):
        data = self.metadata
        # Upstream security.c rounds the disk security table length to eight bytes.
        stack = [((u32(data, 0) + 7) & ~7, '', False)]
        visited = set()
        while stack:
            cursor, parent, siblings = stack.pop()
            while True:
                if cursor in visited or len(visited) >= 250000:
                    raise ValueError('cyclic/oversized metadata tree')
                visited.add(cursor)
                length = u64(data, cursor)
                if length == 0:
                    break
                if length < 104 or length % 8 or cursor + length > len(data):
                    raise ValueError('invalid dentry record')
                count = u16(data, cursor + 96)
                name_size = u16(data, cursor + 100)
                name = data[cursor + 102:cursor + 102 + name_size].decode('utf-16-le', 'surrogatepass')
                path = parent + '/' + name if name else ''
                extra = []
                next_cursor = cursor + length
                for _ in range(count):
                    span = u64(data, next_cursor)
                    namesz = u16(data, next_cursor + 36)
                    if span < 40 or span % 8 or next_cursor + span > len(data) or 38 + namesz > span:
                        raise ValueError('invalid extra stream')
                    extra.append((data[next_cursor + 16:next_cursor + 36], data[next_cursor + 38:next_cursor + 38 + namesz].decode('utf-16-le', 'surrogatepass')))
                    next_cursor += span
                attrs = u32(data, cursor + 8)
                child = u64(data, cursor + 16)
                if child:
                    if attrs & 0x410 != 0x10:
                        raise ValueError('non-directory has child-list offset')
                    stack.append((child, path, True))
                yield path, attrs, data[cursor + 64:cursor + 84], extra, u64(data, cursor + 88), data[cursor:cursor + length]
                if not siblings:
                    break
                cursor = next_cursor


def reparse_fields(tag, data):
    if tag not in (0xa0000003, 0xa000000c):
        raise ValueError('fixture reparse tag unsupported')
    start = 12 if tag == 0xa000000c else 8
    so, sl, po, pl = struct.unpack_from('<HHHH', data)
    decode = lambda offset, size: data[start + offset:start + offset + size].decode('utf-16-le')
    normalize = lambda text: text.replace('X:\\NativeDiskCapture', '<ROOT>').replace('C:\\NativeDiskCapture', '<ROOT>')
    return {'Tag': tag, 'Sub': normalize(decode(so, sl)), 'Print': normalize(decode(po, pl)), 'Flags': u32(data, 8) if tag == 0xa000000c else 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wim', type=Path)
    parser.add_argument('--library', required=True, type=Path)
    parser.add_argument('--source-metadata', required=True, type=Path)
    parser.add_argument('--setup-system-drive', choices=['C:'],
                        help='Require internal fixture absolute links to retain final C: targets and NOT_FIXED for Setup.')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--absent-path', action='append', default=[],
                        help='Fail if a deployment artifact exists (repeatable explicit path)')
    args = parser.parse_args()
    source = json.loads(args.source_metadata.read_text())
    if isinstance(source, dict) and 'stdout' in source:
        source = json.loads(source['stdout'])
    expected = {'/NativeDiskCapture' + row['Path'].replace('\\', '/'): row for row in source if row['Reparse']}
    archive = Archive(args.wim, args.library)
    evidence = []
    errors = []
    forbidden = {path.replace('\\', '/').casefold() for path in args.absent_path}
    forbidden_found = []
    try:
        for path, attrs, main_hash, extra, inode in archive.nodes():
            if path.casefold() in forbidden:
                forbidden_found.append(path)
                errors.append(path + ': forbidden deployment artifact present')
            if not attrs & 0x400:
                continue
            # Canonical writer rules: with extra streams, main_hash is zero and
            # all unnamed streams precede the named ones; reparse is first.
            if extra:
                unnamed = 0
                seen_named = False
                for _, name in extra:
                    if name:
                        seen_named = True
                    elif seen_named:
                        errors.append(path + ': unnamed stream follows named stream')
                    else:
                        unnamed += 1
                if main_hash != ZERO or extra[0][0] == ZERO or extra[0][1] or unnamed != (1 if attrs & 0x10 else 2):
                    errors.append(path + ': noncanonical reparse stream slots')
                rp_hash = extra[0][0]
            else:
                rp_hash = main_hash
            item = {'path': path, 'main_hash': main_hash.hex(), 'extra_streams': [{'hash': h.hex(), 'name': n} for h, n in extra], 'tag': inode & 0xffffffff}
            item['not_fixed'] = bool((inode >> 48) & 1)
            if path in expected:
                baseline = expected[path]
                try:
                    item['reparse'] = reparse_fields(item['tag'], archive.blob(rp_hash))
                    if args.setup_system_drive and item['reparse']['Flags'] == 0:
                        payload = archive.blob(rp_hash)
                        so, sl, _, _ = struct.unpack_from('<HHHH', payload)
                        start = 12 if item['tag'] == 0xa000000c else 8
                        substitute = payload[start + so:start + so + sl].decode('utf-16-le')
                        item['raw_substitute'] = substitute
                        if not item['not_fixed'] or not substitute.casefold().startswith('\\??\\c:\\'):
                            errors.append(path + ': Setup absolute link must retain final C: target and NOT_FIXED')
                    if item['reparse'] != baseline['Reparse']:
                        errors.append(path + ': reparse payload differs from source')
                except (ValueError, struct.error, UnicodeError, KeyError) as error:
                    errors.append(path + ': invalid reparse payload: ' + str(error))
                actual = extra[1:] if extra else []
                for stream in baseline['Streams']:
                    name = stream['Name']
                    found = next((h for h, n in actual if n == name), None)
                    if found is None:
                        errors.append(path + ': source data stream missing: ' + repr(name))
                        continue
                    payload = archive.blob(found)
                    actual_hash = hashlib.sha256(payload).hexdigest() if payload else ''
                    if len(payload) != stream['Length'] or actual_hash != stream['Hash'].replace('-', '').lower():
                        errors.append(path + ': source stream payload differs: ' + repr(name))
                if len(actual) != len(baseline['Streams']):
                    errors.append(path + ': source stream count differs')
                item['fixture_verified'] = not any(error.startswith(path + ':') for error in errors)
            evidence.append(item)
    finally:
        archive.close()
    missing = set(expected) - {item['path'] for item in evidence}
    errors.extend('missing fixture reparse ' + path for path in sorted(missing))
    output = {'oracle': 'independent Python on-disk metadata parser + upstream libwim decompressor', 'passes': not errors, 'errors': errors, 'reparse_entries': evidence, 'setup_system_drive': args.setup_system_drive, 'forbidden_paths': args.absent_path, 'forbidden_paths_found': forbidden_found, 'library': str(args.library), 'limitations': ['Checks ordinary nonsolid single-image WIMs only; decoder library is an independent test oracle, never production conversion.']}
    args.output.write_text(json.dumps(output, indent=2) + '\n')
    print(json.dumps({'passes': output['passes'], 'errors': errors, 'reparse_entries': len(evidence)}, indent=2))
    raise SystemExit(0 if output['passes'] else 1)


if __name__ == '__main__':
    main()
