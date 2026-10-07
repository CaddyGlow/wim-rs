//! Ownership and text-conversion regressions for the alloc-based format core.
use wim_format::{ParseError, xml::XmlInfo};

#[test]
fn xml_clone_preserves_nested_properties_after_source_is_dropped() {
    let input = br#"<WIM VERSION="1"><TOTALBYTES>42</TOTALBYTES><IMAGE INDEX="1"><NAME>alpha</NAME><CUSTOM X="attribute">before<NESTED>inside</NESTED>after</CUSTOM></IMAGE><IMAGE INDEX="2"><NAME>beta</NAME></IMAGE></WIM>"#;
    let xml = XmlInfo::parse_bytes(input).unwrap();
    let expected = xml.encode_utf16le().unwrap();
    let cloned = xml.try_clone().unwrap();
    drop(xml);
    assert_eq!(cloned.encode_utf16le().unwrap(), expected);
}

#[test]
fn lookup_descriptors_remain_searchable_after_archive_is_moved() {
    let file = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/evidence/default-cpu-crash-original.wim"
    ))
    .unwrap();
    let archive = wim_format::archive::Archive::open(&file).unwrap();
    let hashes: Vec<_> = archive.lookup.blobs.iter().map(|blob| blob.hash).collect();
    let moved = Box::new(archive);
    assert!(!hashes.is_empty());
    for hash in hashes {
        assert_eq!(moved.lookup.find(&hash).unwrap().hash, hash);
    }
}

#[test]
fn metadata_graph_has_stable_parent_inode_and_child_relationships() {
    let file = include_bytes!("fixtures/wim-format/pipable-resource.wim");
    let mut reader = wim_format::pipable_read::PipableReader::new(file.as_slice()).unwrap();
    let image = wim_format::pipable_image::read_image(&mut reader, Some(b"1")).unwrap();
    let tree = wim_format::metadata::Metadata::parse(&image.metadata).unwrap();
    let expected: Vec<_> = tree
        .nodes
        .iter()
        .map(|node| {
            (
                node.parent,
                node.inode,
                node.entry.name.to_vec(),
                node.children.to_vec(),
            )
        })
        .collect();
    assert!(!expected.is_empty());
    drop(tree);
    let reparsed = wim_format::metadata::Metadata::parse(&image.metadata).unwrap();
    assert_eq!(
        reparsed
            .nodes
            .iter()
            .map(|node| (
                node.parent,
                node.inode,
                node.entry.name.to_vec(),
                node.children.to_vec()
            ))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn platform_conversion_preserves_wtf8_and_rejects_invalid_text() {
    use wim_format::platform_text::{utf16le_to_wtf8z, wtf8_to_utf16z};
    let input = b"a\xed\xa0\x80\xf0\x90\x80\x80";
    let units = wtf8_to_utf16z(input).unwrap();
    assert_eq!(units.as_slice(), [b'a' as u16, 0xd800, 0xd800, 0xdc00, 0]);
    let bytes: Vec<_> = units[..units.len() - 1]
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    assert_eq!(
        utf16le_to_wtf8z(&bytes).unwrap(),
        [input.as_slice(), &[0]].concat()
    );
    assert_eq!(
        wtf8_to_utf16z(b"\xff").unwrap_err(),
        ParseError::InvalidUtf8String
    );
    assert_eq!(
        utf16le_to_wtf8z(&[0]).unwrap_err(),
        ParseError::InvalidUtf16String
    );
}
