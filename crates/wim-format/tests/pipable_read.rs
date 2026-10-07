#![cfg(feature = "std")]
use std::io::{self, Read};
use wim_format::{
    ParseError,
    archive::Archive,
    file_resource::FileReadError,
    pipable_read::{BlobHeader, Frame, PipableReader},
};

struct Fragments<'a> {
    bytes: &'a [u8],
    maximum: usize,
    calls: usize,
}
impl Read for Fragments<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        // Interrupted reads must not advance the source or become EOF.
        if self.calls.is_multiple_of(13) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let length = output.len().min(self.bytes.len()).min(self.maximum);
        output[..length].copy_from_slice(&self.bytes[..length]);
        self.bytes = &self.bytes[length..];
        Ok(length)
    }
}
fn blob<R: Read>(reader: &mut PipableReader<R>) -> BlobHeader {
    match reader.next_frame(false).unwrap() {
        Frame::Blob(blob) => blob,
        Frame::Part(_) => panic!("unexpected part header"),
    }
}
fn payload<R: Read>(reader: &mut PipableReader<R>) -> BlobHeader {
    let xml = blob(reader);
    assert_ne!(xml.flags & 2, 0);
    reader.skip_resource(xml).unwrap();
    for _ in 0..reader.header().image_count {
        let metadata = blob(reader);
        assert_ne!(metadata.flags & 2, 0);
        reader
            .read_resource(metadata, true, false, |_| Ok(()))
            .unwrap();
    }
    blob(reader)
}
const FIXTURES: [&[u8]; 4] = [
    include_bytes!("fixtures/pipe-none.wim"),
    include_bytes!("fixtures/pipable-resource.wim"),
    include_bytes!("fixtures/pipe-lzx.wim"),
    include_bytes!("fixtures/pipe-lzms.wim"),
];

#[test]
fn original_all_codec_frames_decode_with_fragmented_nonseekable_input() {
    for fixture in FIXTURES {
        let archive = Archive::open(fixture).unwrap();
        let expected = archive.read_blob(&archive.lookup.blobs[0].hash).unwrap();
        let mut original_payload: Vec<_> = (0u8..=255).cycle().take(76800).collect();
        original_payload.extend_from_slice(b"last chunk");
        assert_eq!(expected, original_payload);
        for maximum in [1, 7, 4096] {
            let input = Fragments {
                bytes: fixture,
                maximum,
                calls: 0,
            };
            let mut reader = PipableReader::new(input).unwrap();
            assert_eq!(reader.bytes_read(), 208);
            let header = payload(&mut reader);
            let mut actual = Vec::new();
            reader
                .read_resource(header, true, false, |chunk| {
                    actual.extend_from_slice(chunk);
                    Ok(())
                })
                .unwrap();
            assert_eq!(actual, expected);
            let resource = archive.lookup.resources[archive.lookup.blobs[0].resource_index].header;
            assert_eq!(
                reader.bytes_read(),
                resource.offset_in_wim + resource.size_in_wim
            );
            // Selected-data completion must leave lookup/XML/final-header unread.
            assert!(reader.bytes_read() < fixture.len() as u64);
            let next = blob(&mut reader);
            assert_ne!(next.flags & 2, 0);
        }
    }
}

#[test]
fn callback_abort_reads_neither_remaining_chunks_nor_trailer() {
    for fixture in FIXTURES {
        let mut reader = PipableReader::new(fixture).unwrap();
        let header = payload(&mut reader);
        let start = reader.bytes_read();
        let chunk_size = if header.flags & 4 == 0 {
            32768
        } else {
            reader.header().chunk_size as usize
        };
        let first_size = if header.flags & 4 == 0 {
            chunk_size as u64
        } else {
            4 + u32::from_le_bytes(
                fixture[start as usize..start as usize + 4]
                    .try_into()
                    .unwrap(),
            ) as u64
        };
        let error = reader
            .read_resource(header, true, false, |_| Err(ParseError::AbortedByProgress))
            .unwrap_err();
        assert!(matches!(
            error,
            FileReadError::Format(ParseError::AbortedByProgress)
        ));
        assert_eq!(reader.bytes_read(), start + first_size);
        assert!(reader.next_frame(true).is_err());
        assert_eq!(
            reader.into_inner().len(),
            fixture.len() - (start + first_size) as usize
        );
    }
}

#[test]
fn truncation_preserves_exact_consumed_position_and_does_not_mask_io_errors() {
    let fixture = FIXTURES[1];
    let mut full = PipableReader::new(fixture).unwrap();
    let header = payload(&mut full);
    let start = full.bytes_read() as usize;
    full.skip_resource(header).unwrap();
    let end = full.bytes_read() as usize;
    for length in [
        0,
        1,
        207,
        208,
        209,
        247,
        start,
        start + 1,
        start + 4,
        end - 1,
    ] {
        let result = (|| {
            let mut reader = PipableReader::new(&fixture[..length])?;
            while reader.bytes_read() < start as u64 {
                let frame = reader.next_frame(false)?;
                let Frame::Blob(blob) = frame else {
                    unreachable!()
                };
                reader.skip_resource(blob)?;
            }
            let result = reader.skip_resource(header);
            assert_eq!(reader.bytes_read(), length as u64);
            result
        })();
        assert!(
            matches!(
                result,
                Err(FileReadError::Format(ParseError::UnexpectedEndOfFile))
            ),
            "cut at {length}: {result:?}"
        );
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from_raw_os_error(9))
        }
    }
    assert!(
        matches!(PipableReader::new(Broken), Err(FileReadError::Io(error)) if error.raw_os_error() == Some(9))
    );
}

#[test]
fn part_header_accepts_raw_identity_only_where_requested() {
    let fixture = FIXTURES[1];
    let mut input = fixture[..208].to_vec();
    let mut part = fixture[..208].to_vec();
    part[40..44].copy_from_slice(&[0, 0, 0, 0]);
    part[8..12].fill(0); // Later headers are not subjected to open-time validation.
    input.extend_from_slice(&part);
    let mut reader = PipableReader::new(input.as_slice()).unwrap();
    let Frame::Part(header) = reader.next_frame(true).unwrap() else {
        panic!("expected part")
    };
    assert_eq!(header.part_number, 0);
    assert_eq!(header.total_parts, 0);
    assert_eq!(reader.bytes_read(), 416);
    let mut reader = PipableReader::new(input.as_slice()).unwrap();
    assert!(matches!(
        reader.next_frame(false),
        Err(FileReadError::Format(ParseError::InvalidPipableWim))
    ));
    assert_eq!(reader.bytes_read(), 248);
}

#[test]
fn huge_raw_resource_uses_one_bounded_buffer() {
    struct Zeros<'a> {
        prefix: &'a [u8],
        requested: usize,
    }
    impl Read for Zeros<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.requested = self.requested.max(output.len());
            if self.prefix.is_empty() {
                output.fill(0);
                return Ok(output.len());
            }
            let length = output.len().min(self.prefix.len());
            output[..length].copy_from_slice(&self.prefix[..length]);
            self.prefix = &self.prefix[length..];
            Ok(length)
        }
    }
    let mut prefix = FIXTURES[0][..208].to_vec();
    prefix.extend_from_slice(&0x2b9b9ba2443db9d8u64.to_le_bytes());
    prefix.extend_from_slice(&(1u64 << 40).to_le_bytes());
    prefix.extend_from_slice(&[0; 24]);
    let prefix = prefix.as_slice();
    let mut reader = PipableReader::new(Zeros {
        prefix,
        requested: 0,
    })
    .unwrap();
    let header = blob(&mut reader);
    let error = reader
        .read_resource(header, false, false, |bytes| {
            assert_eq!(bytes.len(), 32768);
            Err(ParseError::AbortedByProgress)
        })
        .unwrap_err();
    assert!(matches!(
        error,
        FileReadError::Format(ParseError::AbortedByProgress)
    ));
    assert_eq!(reader.bytes_read(), 248 + 32768);
    assert_eq!(reader.into_inner().requested, 32768);
}

#[test]
fn unselected_metadata_decodes_chunks_without_parsing_or_verifying_hashes() {
    let fixture = include_bytes!("fixtures/pipe-two-image.wim");
    let archive = Archive::open(fixture).unwrap();
    let descriptor = &archive.lookup.metadata[1];
    let resource = archive.lookup.resources[descriptor.resource_index].header;
    let start = resource.offset_in_wim as usize;
    let length = u32::from_le_bytes(fixture[start..start + 4].try_into().unwrap()) as usize;
    assert!(length < resource.uncompressed_size as usize);
    for (value, verify_hash, expected) in [
        (0, false, None),
        (0, true, Some(ParseError::InvalidResourceHash)),
        (0x11, false, Some(ParseError::Decompression)),
    ] {
        let mut bytes = fixture.to_vec();
        bytes[start + 4..start + 4 + length].fill(value);
        let mut reader = PipableReader::new(bytes.as_slice()).unwrap();
        let xml = blob(&mut reader);
        reader.skip_resource(xml).unwrap();
        let first = blob(&mut reader);
        reader.skip_resource(first).unwrap();
        let second = blob(&mut reader);
        assert_eq!(second.offset, resource.offset_in_wim);
        let result = reader.read_resource(second, verify_hash, false, |_| Ok(()));
        match expected {
            None => {
                result.unwrap();
                assert_eq!(
                    reader.bytes_read(),
                    resource.offset_in_wim + resource.size_in_wim
                );
            }
            Some(expected) => {
                assert!(matches!(result, Err(FileReadError::Format(actual)) if actual == expected))
            }
        }
    }
}

#[test]
fn solid_only_flag_rejects_zero_descriptor_chunk_size_before_resource_data() {
    let mut fixture = FIXTURES[1].to_vec();
    fixture[244..248].copy_from_slice(&18u32.to_le_bytes());
    let mut reader = PipableReader::new(fixture.as_slice()).unwrap();
    let xml = blob(&mut reader);
    assert!(matches!(
        reader.skip_resource(xml),
        Err(FileReadError::Format(ParseError::InvalidChunkSize))
    ));
    assert_eq!(reader.bytes_read(), 248);
}

#[test]
fn large_compressed_chunks_preserve_resource_callback_and_read_boundaries() {
    for fixture in [
        include_bytes!("fixtures/pipe-xpress-64k.wim").as_slice(),
        include_bytes!("fixtures/pipe-lzx-64k.wim").as_slice(),
        include_bytes!("fixtures/pipe-lzms-128k.wim").as_slice(),
    ] {
        let mut reader = PipableReader::new(Fragments {
            bytes: fixture,
            maximum: 7,
            calls: 0,
        })
        .unwrap();
        let header = payload(&mut reader);
        let chunk_size = reader.header().chunk_size as usize;
        assert!(chunk_size > 32768);
        let mut actual = Vec::new();
        let mut sizes = Vec::new();
        reader
            .read_resource(header, true, false, |chunk| {
                sizes.push(chunk.len());
                actual.extend_from_slice(chunk);
                Ok(())
            })
            .unwrap();
        let mut expected: Vec<_> = (0u8..=255).cycle().take(460800).collect();
        expected.extend_from_slice(b"last chunk");
        assert_eq!(actual, expected);
        assert_eq!(sizes[0], chunk_size);
        assert_eq!(sizes.last().copied(), Some(expected.len() % chunk_size));
        let mut reader = PipableReader::new(fixture).unwrap();
        let header = payload(&mut reader);
        let start = reader.bytes_read() as usize;
        let size = u32::from_le_bytes(fixture[start..start + 4].try_into().unwrap()) as usize;
        assert!(matches!(
            reader.read_resource(header, true, false, |chunk| {
                assert_eq!(chunk.len(), chunk_size);
                Err(ParseError::AbortedByProgress)
            }),
            Err(FileReadError::Format(ParseError::AbortedByProgress))
        ));
        assert_eq!(reader.bytes_read(), (start + 4 + size) as u64);
    }
}
