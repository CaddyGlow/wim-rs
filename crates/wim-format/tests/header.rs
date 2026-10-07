use wim_format::{Compression, Header, ParseError, ResourceHeader};
const FIXTURE: &[u8] = include_bytes!("fixtures/empty_dacl.header");

#[test]
fn seekable_pipable_uses_final_header() {
    let file = include_bytes!("fixtures/pipable-resource.wim");
    let expected = Header::parse(&file[file.len() - 208..], Some(file.len() as u64)).unwrap();
    assert_eq!(Header::parse_seekable(file), Ok(expected));
}

#[test]
fn seekable_pipable_ignores_placeholder_header_fields() {
    let mut file = include_bytes!("fixtures/pipable-resource.wim").to_vec();
    file[8..24].fill(0);
    let expected = Header::parse(&file[file.len() - 208..], Some(file.len() as u64)).unwrap();
    assert_eq!(Header::parse_seekable(&file), Ok(expected));
}

#[test]
fn seekable_pipable_retains_initial_magic_without_validating_final_magic() {
    let mut file = include_bytes!("fixtures/pipable-resource.wim").to_vec();
    let expected = Header::parse(&file[file.len() - 208..], Some(file.len() as u64)).unwrap();
    let end_header = file.len() - 208;
    file[end_header..end_header + 8].fill(0xfa);
    assert_eq!(Header::parse_seekable(&file), Ok(expected));
}
#[test]
fn upstream_header_roundtrips() {
    let h = Header::parse(FIXTURE, None).unwrap();
    assert_eq!(h.version, 0x10d00);
    assert_eq!(h.flags, 128);
    assert_eq!(h.chunk_size, 0);
    assert_eq!(
        h.blob_table,
        ResourceHeader {
            size_in_wim: 100,
            flags: 2,
            offset_in_wim: 4666,
            uncompressed_size: 100
        }
    );
    assert_eq!(h.image_count, 1);
    assert_eq!(h.part_number, 1);
    assert_eq!(h.encode(), FIXTURE);
}
#[test]
fn every_truncated_header_returns_eof() {
    for n in 0..208 {
        assert_eq!(
            Header::parse(&FIXTURE[..n], None),
            Err(ParseError::UnexpectedEndOfFile)
        );
    }
}
#[test]
fn basic_field_errors_match_upstream_order() {
    for (offset, value, error) in [
        (8, 207u32, ParseError::InvalidHeader),
        (12, 1, ParseError::UnknownVersion),
        (44, 65536, ParseError::ImageCount),
    ] {
        let mut b = FIXTURE.to_vec();
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(Header::parse(&b, None), Err(error));
    }
    let mut b = FIXTURE.to_vec();
    b[40..42].fill(0);
    assert_eq!(Header::parse(&b, None), Err(ParseError::InvalidPartNumber));
    b[0] = 0;
    assert_eq!(Header::parse(&b, None), Err(ParseError::NotAWimFile));
}
#[test]
fn file_size_guard_and_zero_image_count_match_upstream() {
    assert_eq!(
        Header::parse(FIXTURE, Some(1)),
        Err(ParseError::InvalidHeader)
    );
    let mut b = FIXTURE.to_vec();
    b[44..48].fill(0);
    b[120..124].copy_from_slice(&99u32.to_le_bytes());
    assert_eq!(Header::parse(&b, None).unwrap().image_count, 0);
    assert!(Header::parse(FIXTURE, Some(0)).is_ok());
}
#[test]
fn resource_uses_56_bit_size_and_preserves_unknown_flags() {
    let r = ResourceHeader {
        size_in_wim: 0x00ff_ffff_ffff_ffff,
        flags: 0xab,
        offset_in_wim: u64::MAX,
        uncompressed_size: 77,
    };
    let bytes = r.encode();
    assert_eq!(bytes[7], 0xab);
    assert_eq!(ResourceHeader::parse(&bytes).unwrap(), r);
    assert_eq!(
        ResourceHeader::parse(&bytes[..23]),
        Err(ParseError::UnexpectedEndOfFile)
    );
}
#[test]
fn compression_precedence_and_chunk_bounds_match_c() {
    let mut h = Header::parse(FIXTURE, None).unwrap();
    h.flags = 2 | 0x40000 | 0x20000 | 0x80000;
    h.chunk_size = 32768;
    assert_eq!(h.validate_compression(), Ok(Compression::Lzx));
    h.flags = 2 | 0x200000;
    h.chunk_size = 4096;
    assert_eq!(h.validate_compression(), Ok(Compression::Xpress));
    h.chunk_size = 8193;
    assert_eq!(h.validate_compression(), Err(ParseError::InvalidChunkSize));
    h.flags = 2;
    assert_eq!(
        h.validate_compression(),
        Err(ParseError::InvalidCompressionType)
    );
    h.flags = 0;
    h.chunk_size = 0;
    assert_eq!(h.validate_compression(), Ok(Compression::None));
}
#[test]
fn reserved_bytes_roundtrip_but_canonical_writer_zeros_them() {
    let mut b = FIXTURE.to_vec();
    b[148..].fill(0xac);
    let h = Header::parse(&b, None).unwrap();
    assert_eq!(h.encode().as_slice(), b);
    assert!(h.encode_canonical()[148..].iter().all(|&x| x == 0));
}

#[test]
fn pipable_magic_and_noncanonical_flags_are_not_rejected() {
    let mut b = FIXTURE.to_vec();
    b[..8].copy_from_slice(b"WLPWM\0\0\0");
    b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Header::parse(&b, None).is_ok());
}
