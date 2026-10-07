use wim_format::{
    Compression, HEADER_SIZE, PIPABLE_MAGIC, archive::Archive,
    pipable_write::write_pipable_archive, repack::WriteOptions,
};
#[test]
fn pipable_resources_roundtrip_all_native_modes_and_integrity() {
    let input = include_bytes!("fixtures/xpress-resource.wim");
    let source = Archive::open(input).unwrap();
    for compression in [
        Compression::None,
        Compression::Xpress,
        Compression::Lzx,
        Compression::Lzms,
    ] {
        for integrity in [false, true] {
            let bytes = write_pipable_archive(
                &source,
                WriteOptions {
                    compression,
                    chunk_size: if compression == Compression::None {
                        0
                    } else {
                        32768
                    },
                    integrity,
                },
            )
            .unwrap();
            assert_eq!(&bytes[..8], &PIPABLE_MAGIC);
            assert_eq!(
                &bytes[bytes.len() - HEADER_SIZE..bytes.len() - HEADER_SIZE + 8],
                &PIPABLE_MAGIC
            );
            let target = Archive::open(&bytes).unwrap();
            let mut original_xml = source.xml().unwrap();
            let mut written_xml = target.xml().unwrap();
            original_xml.set_total_bytes(None).unwrap();
            written_xml.set_total_bytes(None).unwrap();
            assert_eq!(
                written_xml.to_xml().unwrap(),
                original_xml.to_xml().unwrap()
            );
            assert_eq!(
                target.read_metadata(1).unwrap(),
                source.read_metadata(1).unwrap()
            );
            for blob in &source.lookup.blobs {
                assert_eq!(
                    target.read_blob(&blob.hash).unwrap(),
                    source.read_blob(&blob.hash).unwrap()
                );
            }
            assert_eq!(
                target.check_integrity().unwrap(),
                if integrity {
                    wim_format::integrity::IntegrityStatus::Ok
                } else {
                    wim_format::integrity::IntegrityStatus::Nonexistent
                }
            );
        }
    }
}

#[test]
fn preliminary_header_omits_seekable_descriptors_and_metadata_precedes_payloads() {
    let input = include_bytes!("fixtures/xpress-resource.wim");
    let mut source = Archive::open(input).unwrap();
    source.header.boot_index = 1;
    let bytes = write_pipable_archive(
        &source,
        WriteOptions {
            compression: Compression::None,
            chunk_size: 0,
            integrity: false,
        },
    )
    .unwrap();
    let initial = wim_format::Header::parse(&bytes[..HEADER_SIZE], None).unwrap();
    assert_eq!(initial.blob_table, wim_format::ResourceHeader::default());
    assert_eq!(initial.xml_data, wim_format::ResourceHeader::default());
    assert_eq!(initial.boot_metadata, wim_format::ResourceHeader::default());
    let output = Archive::open(&bytes).unwrap();
    let metadata = &output.lookup.resources[output.lookup.metadata[0].resource_index].header;
    assert_eq!(output.header.boot_metadata, *metadata);
    for blob in &output.lookup.blobs {
        assert!(
            metadata.offset_in_wim
                < output.lookup.resources[blob.resource_index]
                    .header
                    .offset_in_wim
        );
    }
    let xml_size =
        u64::from_le_bytes(bytes[HEADER_SIZE + 8..HEADER_SIZE + 16].try_into().unwrap()) as usize;
    let mut preliminary = source.xml().unwrap();
    preliminary.set_total_bytes(None).unwrap();
    assert_eq!(
        &bytes[HEADER_SIZE + 40..HEADER_SIZE + 40 + xml_size],
        preliminary.encode_utf16le().unwrap()
    );
}

#[test]
fn refuses_split_and_missing_referenced_content() {
    let input = include_bytes!("fixtures/xpress-resource.wim");
    let mut source = Archive::open(input).unwrap();
    let options = WriteOptions {
        compression: Compression::None,
        chunk_size: 0,
        integrity: false,
    };
    source.header.total_parts = 2;
    assert_eq!(
        write_pipable_archive(&source, options),
        Err(wim_format::ParseError::IsSplitWim)
    );
    source.header.total_parts = 1;
    source.lookup.blobs.clear();
    assert_eq!(
        write_pipable_archive(&source, options),
        Err(wim_format::ParseError::ResourceNotFound)
    );
}
