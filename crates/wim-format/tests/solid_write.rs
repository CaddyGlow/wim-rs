use wim_format::{
    Compression, ParseError, archive::Archive, repack::WriteOptions,
    solid_write::write_solid_archive,
};
#[test]
fn solid_writer_roundtrips_all_source_layouts_and_compressors() {
    for input in [
        include_bytes!("fixtures/xpress-resource.wim").as_slice(),
        include_bytes!("fixtures/pipable-resource.wim").as_slice(),
        include_bytes!("fixtures/solid-resource.wim").as_slice(),
    ] {
        let source = Archive::open(input).unwrap();
        for compression in [Compression::Xpress, Compression::Lzx, Compression::Lzms] {
            for integrity in [false, true] {
                let output = write_solid_archive(
                    &source,
                    WriteOptions {
                        compression,
                        chunk_size: 32768,
                        integrity,
                    },
                )
                .unwrap();
                let dest = Archive::open(&output).unwrap();
                assert_eq!(dest.header.version, 0xe00);
                assert!(dest.lookup.resources.iter().any(|r| r.solid));
                assert!(
                    dest.lookup
                        .metadata
                        .iter()
                        .all(|b| !dest.lookup.resources[b.resource_index].solid)
                );
                let mut original_xml = source.xml().unwrap();
                let mut written_xml = dest.xml().unwrap();
                original_xml.set_total_bytes(None).unwrap();
                written_xml.set_total_bytes(None).unwrap();
                assert_eq!(
                    written_xml.to_xml().unwrap(),
                    original_xml.to_xml().unwrap()
                );
                assert_eq!(
                    dest.read_metadata(1).unwrap(),
                    source.read_metadata(1).unwrap()
                );
                for blob in &source.lookup.blobs {
                    assert_eq!(
                        dest.read_blob(&blob.hash).unwrap(),
                        source.read_blob(&blob.hash).unwrap()
                    );
                    assert_eq!(
                        dest.lookup.find(&blob.hash).unwrap().reference_count,
                        blob.reference_count
                    );
                }
                assert_eq!(
                    dest.check_integrity().unwrap(),
                    if integrity {
                        wim_format::integrity::IntegrityStatus::Ok
                    } else {
                        wim_format::integrity::IntegrityStatus::Nonexistent
                    }
                );
            }
        }
    }
}
#[test]
fn solid_writer_rejects_none_compression_and_split_source() {
    let mut source = Archive::open(include_bytes!("fixtures/xpress-resource.wim")).unwrap();
    assert_eq!(
        write_solid_archive(
            &source,
            WriteOptions {
                compression: Compression::None,
                chunk_size: 0,
                integrity: false
            }
        ),
        Err(ParseError::InvalidCompressionType)
    );
    source.header.total_parts = 2;
    assert_eq!(
        write_solid_archive(
            &source,
            WriteOptions {
                compression: Compression::Xpress,
                chunk_size: 32768,
                integrity: false
            }
        ),
        Err(ParseError::IsSplitWim)
    );
}
#[test]
fn solid_writer_preserves_boot_metadata_and_rejects_missing_content() {
    let mut source = Archive::open(include_bytes!("fixtures/xpress-resource.wim")).unwrap();
    source.header.boot_index = 1;
    let options = WriteOptions {
        compression: Compression::Lzx,
        chunk_size: 32768,
        integrity: true,
    };
    let bytes = write_solid_archive(&source, options).unwrap();
    let output = Archive::open(&bytes).unwrap();
    assert_eq!(output.header.boot_index, 1);
    assert_eq!(
        output.header.boot_metadata,
        output.lookup.resources[output.lookup.metadata[0].resource_index].header
    );
    source.lookup.blobs.clear();
    assert_eq!(
        write_solid_archive(&source, options),
        Err(ParseError::ResourceNotFound)
    );
}
