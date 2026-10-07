use wim_format::{
    Compression, ParseError,
    archive::Archive,
    repack::WriteOptions,
    split_join::{join_archives, split_archive},
};
const SOURCE: &[u8] = include_bytes!("fixtures/xpress-resource.wim");
fn options() -> WriteOptions {
    WriteOptions {
        compression: Compression::Xpress,
        chunk_size: 32768,
        integrity: true,
    }
}
#[test]
fn split_keeps_metadata_only_in_first_and_join_accepts_reverse_order() {
    let original = Archive::open(SOURCE).unwrap();
    let parts = split_archive(SOURCE, 1, [42; 16], true).unwrap();
    assert_eq!(parts.len(), 2);
    for (i, bytes) in parts.iter().enumerate() {
        let part = Archive::open(bytes).unwrap();
        assert_eq!(part.header.part_number, i as u16 + 1);
        assert_eq!(part.header.total_parts, 2);
        assert_eq!(part.header.guid, [42; 16]);
        assert_eq!(part.lookup.metadata.len(), if i == 0 { 1 } else { 0 });
    }
    let joined = join_archives(&[parts[1].as_slice(), parts[0].as_slice()], options()).unwrap();
    let archive = Archive::open(&joined).unwrap();
    assert_eq!(
        archive.read_blob(&original.lookup.blobs[0].hash).unwrap(),
        original.read_blob(&original.lookup.blobs[0].hash).unwrap()
    );
    assert_eq!(archive.header.guid, [42; 16]);
}
#[test]
fn join_rejects_missing_duplicate_and_foreign_parts() {
    let parts = split_archive(SOURCE, 1, [42; 16], false).unwrap();
    assert_eq!(
        join_archives(&[&parts[0]], options()),
        Err(ParseError::SplitInvalid)
    );
    assert_eq!(
        join_archives(&[&parts[0], &parts[0]], options()),
        Err(ParseError::SplitInvalid)
    );
    let other = split_archive(SOURCE, 1, [43; 16], false).unwrap();
    assert_eq!(
        join_archives(&[&parts[0], &other[1]], options()),
        Err(ParseError::SplitInvalid)
    );
    assert_eq!(join_archives(&[], options()), Err(ParseError::InvalidParam));
}
#[test]
fn split_rejects_zero_target_and_solid_resources() {
    assert_eq!(
        split_archive(SOURCE, 0, [42; 16], false),
        Err(ParseError::InvalidParam)
    );
    assert_eq!(
        split_archive(
            include_bytes!("fixtures/solid-resource.wim"),
            10,
            [42; 16],
            false
        ),
        Err(ParseError::Unsupported)
    );
}
#[test]
fn boundary_counts_encoded_payload_not_headers_or_xml() {
    let original = Archive::open(SOURCE).unwrap();
    let size = original
        .lookup
        .metadata
        .iter()
        .chain(original.lookup.blobs.iter())
        .map(|b| {
            original.lookup.resources[b.resource_index]
                .header
                .size_in_wim
        })
        .sum::<u64>();
    assert_eq!(
        split_archive(SOURCE, size, [42; 16], false).unwrap().len(),
        2
    );
    assert_eq!(
        split_archive(SOURCE, size + 1, [42; 16], false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn split_marks_spanned_parts_and_clears_split_boot_selection() {
    let mut bytes = SOURCE.to_vec();
    let mut header = wim_format::Header::parse_seekable(&bytes).unwrap();
    header.boot_index = 1;
    let archive = Archive::open(SOURCE).unwrap();
    header.boot_metadata =
        archive.lookup.resources[archive.lookup.metadata[0].resource_index].header;
    bytes[..wim_format::HEADER_SIZE].copy_from_slice(&header.encode_canonical());
    let parts = split_archive(&bytes, 1, [42; 16], false).unwrap();
    for (index, bytes) in parts.iter().enumerate() {
        let archive = Archive::open(bytes).unwrap();
        assert_ne!(archive.header.flags & 8, 0);
        assert_eq!(archive.header.boot_index, 0);
        assert_eq!(
            archive.header.boot_metadata,
            wim_format::ResourceHeader::default()
        );
        let table = archive.header.blob_table;
        let table_bytes = &bytes
            [table.offset_in_wim as usize..(table.offset_in_wim + table.size_in_wim) as usize];
        for record in table_bytes.chunks_exact(50) {
            assert_eq!(
                wim_format::lookup::LookupEntry::parse(record)
                    .unwrap()
                    .part_number,
                index as u16 + 1
            );
        }
    }
    let joined = join_archives(
        &parts.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        options(),
    )
    .unwrap();
    assert_eq!(Archive::open(&joined).unwrap().header.flags & 8, 0);
}
