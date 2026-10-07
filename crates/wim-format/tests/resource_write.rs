use wim_format::{
    Compression, ParseError,
    resource::{ResourceLayout, read_resource},
    resource_write::encode_resource,
};

#[test]
fn raw_chunks_round_trip_all_layouts_at_chunk_boundaries() {
    for layout in [
        ResourceLayout::Ordinary,
        ResourceLayout::Pipable,
        ResourceLayout::Solid,
    ] {
        for length in [1, 7, 8, 9, 16, 17] {
            let input = vec![42; length];
            let encoded =
                encode_resource(&input, Compression::Xpress, 8, layout, |_, _| Ok(None)).unwrap();
            let decoded = read_resource(
                &encoded.bytes,
                &encoded.header,
                Compression::Xpress,
                8,
                layout,
                |_, _, _| panic!("raw chunk decoded"),
            )
            .unwrap();
            assert_eq!(decoded, input);
        }
    }
}
#[test]
fn ordinary_offsets_exclude_table_bytes() {
    let encoded = encode_resource(
        &[7; 17],
        Compression::Xpress,
        8,
        ResourceLayout::Ordinary,
        |_, chunk| Ok(Some(vec![chunk[0]])),
    )
    .unwrap();
    assert_eq!(encoded.bytes, [1, 0, 0, 0, 2, 0, 0, 0, 7, 7, 7]);
}
#[test]
fn pipable_offsets_exclude_chunk_headers() {
    let encoded = encode_resource(
        &[7; 17],
        Compression::Xpress,
        8,
        ResourceLayout::Pipable,
        |_, chunk| Ok(Some(vec![chunk[0]])),
    )
    .unwrap();
    assert_eq!(
        encoded.bytes,
        [
            1, 0, 0, 0, 7, 1, 0, 0, 0, 7, 1, 0, 0, 0, 7, 1, 0, 0, 0, 2, 0, 0, 0
        ]
    );
}
#[test]
fn solid_table_contains_all_chunk_lengths() {
    let encoded = encode_resource(
        &[7; 17],
        Compression::Xpress,
        8,
        ResourceLayout::Solid,
        |_, chunk| Ok(Some(vec![chunk[0]])),
    )
    .unwrap();
    assert_eq!(
        &encoded.bytes[16..28],
        &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
    );
}
#[test]
fn invalid_chunk_size_rejected_before_compressor() {
    assert_eq!(
        encode_resource(
            &[1],
            Compression::Xpress,
            3,
            ResourceLayout::Ordinary,
            |_, _| panic!("called")
        )
        .unwrap_err(),
        ParseError::InvalidChunkSize
    );
}
#[test]
fn ineffective_compression_uses_raw_chunk() {
    let encoded = encode_resource(
        &[1; 8],
        Compression::Xpress,
        8,
        ResourceLayout::Ordinary,
        |_, _| Ok(Some(vec![2; 9])),
    )
    .unwrap();
    assert_eq!(encoded.bytes, [1; 8]);
}
#[test]
fn none_compression_avoids_chunk_framing() {
    let encoded = encode_resource(
        &[1; 9],
        Compression::None,
        0,
        ResourceLayout::Pipable,
        |_, _| panic!("called"),
    )
    .unwrap();
    assert_eq!((encoded.bytes, encoded.header.flags), (vec![1; 9], 0));
}
#[test]
fn empty_resource_is_uncompressed() {
    let encoded = encode_resource(
        &[],
        Compression::Xpress,
        8,
        ResourceLayout::Solid,
        |_, _| panic!("called"),
    )
    .unwrap();
    assert_eq!((encoded.bytes.len(), encoded.header.flags), (0, 0));
}
#[test]
fn compressor_error_is_propagated() {
    assert_eq!(
        encode_resource(
            &[1],
            Compression::Xpress,
            8,
            ResourceLayout::Ordinary,
            |_, _| Err(ParseError::Write)
        )
        .unwrap_err(),
        ParseError::Write
    );
}
