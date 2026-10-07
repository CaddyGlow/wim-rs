use wim_format::{
    Compression,
    archive::Archive,
    pipable_write::write_pipable_archive,
    repack::{WriteOptions, write_archive},
    solid_write::write_solid_archive,
};
#[test]
fn all_layouts_report_new_stored_bytes_before_xml() {
    let source = Archive::open(include_bytes!("fixtures/xpress-resource.wim")).unwrap();
    let options = WriteOptions {
        compression: Compression::Xpress,
        chunk_size: 32768,
        integrity: true,
    };
    for bytes in [
        write_archive(&source, options).unwrap(),
        write_solid_archive(&source, options).unwrap(),
        write_pipable_archive(&source, options).unwrap(),
    ] {
        let output = Archive::open(&bytes).unwrap();
        assert_eq!(
            output.xml().unwrap().total_bytes(),
            output.header.blob_table.offset_in_wim + output.header.blob_table.size_in_wim
        );
    }
}

#[test]
fn pipable_preliminary_xml_omits_stored_byte_statistics() {
    let source = Archive::open(include_bytes!("fixtures/xpress-resource.wim")).unwrap();
    let bytes = write_pipable_archive(
        &source,
        WriteOptions {
            compression: Compression::Xpress,
            chunk_size: 32768,
            integrity: false,
        },
    )
    .unwrap();
    let mut raw = [0; 8];
    raw.copy_from_slice(&bytes[216..224]);
    let length = u64::from_le_bytes(raw) as usize;
    let xml = wim_format::xml::XmlInfo::parse_utf16le(&bytes[248..248 + length]).unwrap();
    assert_eq!(xml.total_bytes(), 0);
    assert_eq!(xml.name(1), source.xml().unwrap().name(1));
}
