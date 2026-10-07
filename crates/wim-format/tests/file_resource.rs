#![cfg(feature = "std")]
use std::io::{self, Read, Seek, SeekFrom};
use wim_format::{
    Compression, ResourceHeader, archive::Archive, file_resource::read_resource_range,
    resource::ResourceLayout,
};

#[test]
fn solid_cache_reuses_one_chunk_rechecks_mutations_and_rejects_truncation() {
    use std::io::Cursor;
    use wim_format::file_resource::{ChunkCache, read_resource_range_cached};
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&64u64.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[11, 22]);
    let header = ResourceHeader {
        size_in_wim: bytes.len() as u64,
        flags: 16,
        offset_in_wim: 0,
        uncompressed_size: 64,
    };
    let mut file = Cursor::new(bytes);
    let mut cache = ChunkCache::default();
    let calls = std::cell::Cell::new(0);
    let read = |file: &mut Cursor<Vec<u8>>, cache: &mut ChunkCache, range| {
        read_resource_range_cached(
            file,
            &header,
            Compression::Xpress,
            32,
            ResourceLayout::Solid,
            range,
            |_, input, output| {
                calls.set(calls.get() + 1);
                output.fill(input[0]);
                Ok(())
            },
            Some(cache),
        )
    };
    assert_eq!(read(&mut file, &mut cache, 0..4).unwrap(), vec![11; 4]);
    assert_eq!(read(&mut file, &mut cache, 8..12).unwrap(), vec![11; 4]);
    assert_eq!(
        calls.get(),
        1,
        "files sharing a solid chunk must decode it once"
    );
    file.get_mut()[24] = 33;
    assert_eq!(read(&mut file, &mut cache, 8..12).unwrap(), vec![33; 4]);
    assert_eq!(
        calls.get(),
        2,
        "changed input must invalidate decoded bytes"
    );
    assert_eq!(read(&mut file, &mut cache, 32..36).unwrap(), vec![22; 4]);
    assert_eq!(read(&mut file, &mut cache, 0..4).unwrap(), vec![33; 4]);
    assert_eq!(calls.get(), 4, "only the most recent chunk is retained");
    file.get_mut()[24] = 44;
    let failure = read_resource_range_cached(
        &mut file,
        &header,
        Compression::Xpress,
        32,
        ResourceLayout::Solid,
        0..4,
        |_, _, output| {
            output.fill(99);
            Err(wim_format::ParseError::Decompression)
        },
        Some(&mut cache),
    );
    assert!(failure.is_err());
    assert_eq!(
        read(&mut file, &mut cache, 0..4).unwrap(),
        vec![44; 4],
        "failed decoder output is never cached"
    );
    file.get_mut().truncate(25);
    assert!(
        read(&mut file, &mut cache, 0..4).is_err(),
        "cached bytes cannot hide truncation"
    );
}

struct Sparse {
    position: u64,
    size: u64,
    bytes_read: u64,
    max_read: usize,
}
impl Read for Sparse {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let length = out.len().min((self.size - self.position) as usize);
        for (i, byte) in out[..length].iter_mut().enumerate() {
            *byte = (self.position + i as u64) as u8;
        }
        self.position += length as u64;
        self.bytes_read += length as u64;
        self.max_read = self.max_read.max(length);
        Ok(length)
    }
}
impl Seek for Sparse {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = match pos {
            SeekFrom::Start(x) => x,
            SeekFrom::End(x) => self.size.checked_add_signed(x).unwrap(),
            SeekFrom::Current(x) => self.position.checked_add_signed(x).unwrap(),
        };
        Ok(self.position)
    }
}
#[test]
fn sparse_raw_range_reads_only_selected_bytes() {
    let mut file = Sparse {
        position: 0,
        size: 1 << 40,
        bytes_read: 0,
        max_read: 0,
    };
    let header = ResourceHeader {
        flags: 0,
        offset_in_wim: 1 << 38,
        size_in_wim: 1 << 36,
        uncompressed_size: 1 << 36,
    };
    let result = read_resource_range(
        &mut file,
        &header,
        Compression::None,
        0,
        ResourceLayout::Ordinary,
        253..263,
        |_, _, _| panic!("raw decoder"),
    )
    .unwrap();
    assert_eq!(result, [253, 254, 255, 0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(file.bytes_read, 10);
}
#[test]
fn original_layout_fixtures_decode_selected_chunks() {
    for input in [
        include_bytes!("fixtures/xpress-resource.wim").as_slice(),
        include_bytes!("fixtures/pipable-resource.wim").as_slice(),
        include_bytes!("fixtures/solid-resource.wim").as_slice(),
    ] {
        let archive = Archive::open(input).unwrap();
        let blob = &archive.lookup.blobs[0];
        let resource = &archive.lookup.resources[blob.resource_index];
        let layout = if resource.solid {
            ResourceLayout::Solid
        } else if archive.header.magic == wim_format::PIPABLE_MAGIC {
            ResourceLayout::Pipable
        } else {
            ResourceLayout::Ordinary
        };
        let expected = archive.read_blob(&blob.hash).unwrap();
        let mut cache = wim_format::file_resource::ChunkCache::default();
        for selection in [0..1, 32760..32777, 65530..65550, 76800..76810, 0..0] {
            let result = wim_format::file_resource::read_resource_range_cached(
                &mut io::Cursor::new(input),
                &resource.header,
                Compression::Xpress,
                resource.chunk_size,
                layout,
                selection.clone(),
                |_, source, target| {
                    ms_compress::decompress_xpress(source, target)
                        .map_err(|_| wim_format::ParseError::Decompression)
                },
                Some(&mut cache),
            )
            .unwrap();
            assert_eq!(
                result,
                expected[selection.start as usize..selection.end as usize]
            );
        }
    }
}

struct VirtualChunkFile {
    position: u64,
    chunks: u64,
    read_bytes: u64,
    largest_read: usize,
    solid: bool,
}
impl VirtualChunkFile {
    fn prefix(&self) -> u64 {
        if self.solid {
            16 + self.chunks * 4
        } else {
            (self.chunks - 1) * 8
        }
    }
    fn size(&self) -> u64 {
        self.prefix() + self.chunks * 32768
    }
    fn byte(&self, pos: u64) -> u8 {
        if self.solid && pos < 16 {
            let mut h = [0; 16];
            h[..8].copy_from_slice(&(self.chunks * 32768).to_le_bytes());
            h[8..12].copy_from_slice(&32768u32.to_le_bytes());
            h[12..16].copy_from_slice(&1u32.to_le_bytes());
            return h[pos as usize];
        }
        if pos < self.prefix() {
            let pos = pos - if self.solid { 16 } else { 0 };
            if self.solid {
                32768u32.to_le_bytes()[(pos % 4) as usize]
            } else {
                ((pos / 8 + 1) * 32768).to_le_bytes()[(pos % 8) as usize]
            }
        } else {
            (pos - self.prefix()) as u8
        }
    }
}
impl Read for VirtualChunkFile {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let length = out
            .len()
            .min(self.size().saturating_sub(self.position) as usize);
        for (i, byte) in out[..length].iter_mut().enumerate() {
            *byte = self.byte(self.position + i as u64);
        }
        self.position += length as u64;
        self.read_bytes += length as u64;
        self.largest_read = self.largest_read.max(length);
        Ok(length)
    }
}
impl Seek for VirtualChunkFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = match pos {
            SeekFrom::Start(x) => x,
            SeekFrom::End(x) => self.size().checked_add_signed(x).unwrap(),
            SeekFrom::Current(x) => self.position.checked_add_signed(x).unwrap(),
        };
        Ok(self.position)
    }
}
#[test]
fn ordinary_huge_table_reads_only_two_boundaries_and_raw_selection() {
    let mut file = VirtualChunkFile {
        position: 0,
        chunks: 1 << 25,
        read_bytes: 0,
        largest_read: 0,
        solid: false,
    };
    let header = ResourceHeader {
        offset_in_wim: 0,
        flags: 4,
        size_in_wim: file.size(),
        uncompressed_size: file.chunks * 32768,
    };
    let selection = ((1u64 << 24) * 32768 + 253)..((1u64 << 24) * 32768 + 263);
    let output = read_resource_range(
        &mut file,
        &header,
        Compression::Xpress,
        32768,
        ResourceLayout::Ordinary,
        selection,
        |_, _, _| panic!("raw chunk decoder"),
    )
    .unwrap();
    assert_eq!(output, [253, 254, 255, 0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(file.read_bytes, 26);
    assert!(file.largest_read <= 10);
}
#[test]
fn solid_prefix_is_streamed_without_large_table_read() {
    let mut file = VirtualChunkFile {
        position: 0,
        chunks: 32768,
        read_bytes: 0,
        largest_read: 0,
        solid: true,
    };
    let header = ResourceHeader {
        offset_in_wim: 0,
        flags: 16,
        size_in_wim: file.size(),
        uncompressed_size: 0,
    };
    let selection = 32760 * 32768 + 253..32760 * 32768 + 263;
    let output = read_resource_range(
        &mut file,
        &header,
        Compression::None,
        0,
        ResourceLayout::Solid,
        selection,
        |_, _, _| panic!("raw chunk decoder"),
    )
    .unwrap();
    assert_eq!(output, [253, 254, 255, 0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(file.read_bytes, 16 + 32761 * 4 + 10);
    assert!(file.largest_read <= 16);
}
#[test]
fn decreasing_selected_boundaries_fail_without_decoding() {
    let mut bytes = vec![0; 20];
    bytes[..4].copy_from_slice(&5u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
    let header = ResourceHeader {
        offset_in_wim: 0,
        flags: 4,
        size_in_wim: 20,
        uncompressed_size: 24,
    };
    let err = read_resource_range(
        &mut io::Cursor::new(bytes),
        &header,
        Compression::Xpress,
        8,
        ResourceLayout::Ordinary,
        8..9,
        |_, _, _| panic!("invalid decoder"),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        wim_format::file_resource::FileReadError::Format(wim_format::ParseError::Decompression)
    ));
}
#[test]
fn truncated_file_span_fails_before_output_allocation() {
    let header = ResourceHeader {
        offset_in_wim: 0,
        flags: 4,
        size_in_wim: u64::MAX,
        uncompressed_size: u64::MAX,
    };
    let err = read_resource_range(
        &mut io::Cursor::new([0u8; 16]),
        &header,
        Compression::Xpress,
        32768,
        ResourceLayout::Ordinary,
        0..u64::MAX,
        |_, _, _| panic!("invalid decoder"),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        wim_format::file_resource::FileReadError::Format(
            wim_format::ParseError::UnexpectedEndOfFile
        )
    ));
}

#[test]
fn compressed_range_reads_only_intersecting_body() {
    struct Counting {
        inner: io::Cursor<Vec<u8>>,
        bytes: usize,
    }
    impl Read for Counting {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(out)?;
            self.bytes += n;
            Ok(n)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.inner.seek(pos)
        }
    }
    // Three two-byte compressed bodies with an eight-byte ordinary table.
    let mut bytes = vec![0; 14];
    bytes[..4].copy_from_slice(&2u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&4u32.to_le_bytes());
    bytes[8..].copy_from_slice(&[1, 1, 2, 2, 3, 3]);
    let mut file = Counting {
        inner: io::Cursor::new(bytes),
        bytes: 0,
    };
    let header = ResourceHeader {
        offset_in_wim: 0,
        flags: 4,
        size_in_wim: 14,
        uncompressed_size: 24,
    };
    let mut calls = 0;
    let output = read_resource_range(
        &mut file,
        &header,
        Compression::Xpress,
        8,
        ResourceLayout::Ordinary,
        10..13,
        |_, input, out| {
            calls += 1;
            assert_eq!(input, [2, 2]);
            assert_eq!(out.len(), 8);
            out.fill(42);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, [42; 3]);
    assert_eq!(calls, 1);
    assert_eq!(file.bytes, 10);
}
#[test]
fn underlying_seek_error_is_preserved() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl Seek for Broken {
        fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
            Err(io::Error::from_raw_os_error(5))
        }
    }
    let err = read_resource_range(
        &mut Broken,
        &ResourceHeader::default(),
        Compression::None,
        0,
        ResourceLayout::Ordinary,
        0..0,
        |_, _, _| unreachable!(),
    )
    .unwrap_err();
    match err {
        wim_format::file_resource::FileReadError::Io(error) => {
            assert_eq!(error.raw_os_error(), Some(5))
        }
        other => panic!("wrong error: {other}"),
    }
}
