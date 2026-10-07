#![cfg(feature = "std")]

use wim_format::{
    ParseError,
    file_resource::FileReadError,
    metadata::Metadata,
    pipable_image::read_image,
    pipable_read::{Frame, PipableReader},
};

#[test]
fn selection_loads_metadata_for_all_codecs_without_reading_file_payload() {
    for bytes in [
        include_bytes!("fixtures/pipe-none.wim").as_slice(),
        include_bytes!("fixtures/pipable-resource.wim").as_slice(),
        include_bytes!("fixtures/pipe-lzx.wim").as_slice(),
        include_bytes!("fixtures/pipe-lzms.wim").as_slice(),
    ] {
        let mut reader = PipableReader::new(bytes).unwrap();
        let image = read_image(&mut reader, None).unwrap();
        assert_eq!(image.index, 1);
        assert!(!Metadata::parse(&image.metadata).unwrap().nodes.is_empty());
        let consumed = reader.bytes_read();
        let Frame::Blob(payload) = reader.next_frame(false).unwrap() else {
            panic!("expected payload");
        };
        assert_eq!(payload.flags & 2, 0);
        assert_eq!(reader.bytes_read(), consumed + 40);
        assert!(payload.uncompressed_size > 0);
    }
}

#[test]
fn invalid_selector_returns_before_reading_image_metadata() {
    let bytes = include_bytes!("fixtures/pipable-resource.wim");
    let xml_size = u64::from_le_bytes(bytes[216..224].try_into().unwrap());
    for selector in [b"0".as_slice(), b"all", b"missing"] {
        let mut reader = PipableReader::new(bytes.as_slice()).unwrap();
        let result = read_image(&mut reader, Some(selector));
        assert!(matches!(
            result,
            Err(FileReadError::Format(ParseError::InvalidImage))
        ));
        assert_eq!(reader.bytes_read(), 248 + xml_size);
    }
}

#[test]
fn metadata_digest_failure_reports_original_metadata_error() {
    let mut bytes = include_bytes!("fixtures/pipable-resource.wim").to_vec();
    let offset = 248 + u64::from_le_bytes(bytes[216..224].try_into().unwrap()) as usize;
    bytes[offset + 16..offset + 36].fill(0);
    let mut reader = PipableReader::new(bytes.as_slice()).unwrap();
    let result = read_image(&mut reader, Some(b"1"));
    assert!(matches!(
        result,
        Err(FileReadError::Format(ParseError::InvalidMetadataResource))
    ));
}
